use std::ops::{Deref, DerefMut};

use crate::persist::admin::captchastate::CaptchaType;
use crate::persist::core::media::SendMediaReply;
use crate::persist::redis::{
    default_cache_query, CachedQueryTrait, RedisCache, RedisStr, ToRedisStr,
};
use crate::statics::{ME, TG};
use crate::util::error::BotError;
use crate::util::string::Speak;
use crate::{
    langs::Lang,
    persist::{
        admin::{authorized, captchastate},
        core::{media::MediaType, welcomes},
    },
    statics::{CONFIG, DB, REDIS},
    util::error::Result,
};
use base64::engine::general_purpose;
use base64::Engine;
use botapi::gen_types::{
    CallbackQuery, Chat, ChatMemberUpdated, EReplyMarkup, InlineKeyboardButton,
    InlineKeyboardButtonBuilder, MaybeInaccessibleMessage, Message, MessageEntity,
    ReplyParametersBuilder, UpdateExt, User,
};
use captcha::gen;
use chrono::Duration;
use futures::FutureExt;
use macros::lang_fmt;
use rand::rngs::ThreadRng;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use redis::{AsyncCommands, Script};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
use sea_query::OnConflict;
use tokio::time::sleep;
use uuid::Uuid;

use super::admin_helpers::{kick, DeleteAfterTime, UpdateHelpers, UserChanged};
use super::button::{get_url, InlineKeyboardBuilder, OnPush};
use super::command::Context;
use super::markdown::get_markup_for_buttons;
use super::notes::handle_transition;
use super::permissions::{IsAdmin, IsGroupAdmin};
use super::user::{GetChat, Username};

pub(crate) fn auth_key(chat: i64) -> String {
    format!("cauth:{}", chat)
}

/// Loads the cache of users that already completed the captcha from db to redis
pub async fn update_auth_cache(chat: i64) -> Result<()> {
    let key = auth_key(chat);
    if !REDIS.sq(|q| q.exists(&key)).await? {
        let rows = authorized::Entity::find()
            .filter(authorized::Column::Chat.eq(chat))
            .all(DB.deref())
            .await?;

        REDIS
            .pipe(|p| {
                for row in rows {
                    p.sadd(&key, row.user);
                }
                p.expire(&key, CONFIG.timing.cache_timeout)
            })
            .await?;
    }
    Ok(())
}

/// Gets a deep link url for retrieving a captcha from the bot's dm
pub(crate) async fn get_captcha_url(chat: &Chat, user: &User) -> Result<String> {
    let ser = (chat, user).to_redis()?;
    let r = Uuid::new_v4();
    let key = get_callback_key(&r.to_string());
    REDIS
        .pipe(|q| q.set(&key, ser).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    let bs = general_purpose::URL_SAFE_NO_PAD.encode(r.into_bytes());
    let bs = get_url(bs)?;
    Ok(bs)
}

pub(crate) fn get_callback_key(key: &str) -> String {
    format!("ccback:{}", key)
}

/// Returns true if the user has already completed the captcha in the given chat
pub async fn user_is_authorized(chat: i64, user: i64) -> Result<bool> {
    update_auth_cache(chat).await?;
    let key = auth_key(chat);
    REDIS.sq(|q| q.sismember(&key, user)).await
}

fn captcha_state_key(chat: &Chat) -> String {
    format!("cstate:{}", chat.get_id())
}

/// Gets the current captcha configuration for the current update/chat, returns None if captcha is disabled
pub async fn get_captcha_config(
    message: &ChatMemberUpdated,
) -> Result<Option<captchastate::Model>> {
    let key = captcha_state_key(message.get_chat());
    let chat = message.get_chat().get_id();
    let res = default_cache_query(
        |_, _| async move {
            let res = captchastate::Entity::find_by_id(chat)
                .one(DB.deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

pub(crate) async fn goodbye_members(
    ctx: &Context,
    model: welcomes::Model,
    entities: Vec<MessageEntity>,
    buttons: Option<InlineKeyboardBuilder>,
    lang: &Lang,
) -> Result<()> {
    let text = if let Some(text) = model.goodbye_text {
        text
    } else {
        lang_fmt!(lang, "defaultgoodbye")
    };

    SendMediaReply::new(ctx, model.goodbye_media_type.unwrap_or(MediaType::Text))
        .button_callback(|_, _| async move { Ok(()) }.boxed())
        .text(Some(text))
        .media_id(model.goodbye_media_id)
        .extra_entities(entities)
        .buttons(buttons)
        .send_media()
        .await?;
    Ok(())
}

/// Handle sending a welcome message along with a text captcha
pub(crate) async fn welcome_members(
    ctx: &Context,
    upd: &ChatMemberUpdated,
    model: welcomes::Model,
    entities: Vec<MessageEntity>,
    mut extra_buttons: Option<InlineKeyboardBuilder>,
    lang: &Lang,
    captcha: Option<&captchastate::Model>,
) -> Result<()> {
    let text = if let Some(text) = model.text {
        text
    } else {
        lang_fmt!(lang, "defaultwelcome")
    };

    let buttons = if let Some(_) = captcha {
        let url = get_captcha_url(&upd.chat, &upd.from).await?;

        let button = InlineKeyboardButtonBuilder::new("Captcha".to_owned())
            .set_url(url)
            .build();
        vec![button]
    } else {
        vec![]
    };
    let c = ctx.clone();
    let chat = upd.get_chat().get_id();
    if let Some(b) = extra_buttons.as_mut() {
        for button in buttons {
            b.button(button);
        }
    }
    SendMediaReply::new(ctx, model.media_type.unwrap_or(MediaType::Text))
        .button_callback(move |note, button| {
            let c = c.clone();
            async move {
                button.on_push(move |b| async move {
                    TG.client
                        .build_answer_callback_query(b.get_id())
                        .build()
                        .await?;

                    handle_transition(&c, chat, note, b).await?;
                    Ok(())
                });

                Ok(())
            }
            .boxed()
        })
        .text(Some(text))
        .media_id(model.media_id)
        .extra_entities(entities)
        .buttons(extra_buttons)
        .send_media()
        .await?;

    Ok(())
}

fn build_captcha_sync() -> (String, Vec<u8>, Vec<char>) {
    let captcha = gen(captcha::Difficulty::Hard);

    (
        captcha.chars_as_string(),
        captcha.as_png().unwrap(),
        captcha.supported_chars(),
    )
}

#[inline(always)]
fn get_incorrect_counter(callback: &User, incorrect_chat: i64) -> String {
    format!("incc:{}:{}", callback.get_id(), incorrect_chat)
}

/// Clears the counter for incorrect captcha answers for a specific chat and user
async fn reset_incorrect_tries(user: &User, chat: i64) -> Result<()> {
    let key = get_incorrect_counter(user, chat);
    REDIS.sq(|q| q.del(&key)).await
}

/// Atomically increments a redis-backed counter to count incorrect captcha tries
async fn incorrect_tries(callback: &CallbackQuery, incorrect_chat: i64) -> Result<usize> {
    let key = get_incorrect_counter(callback.get_from(), incorrect_chat);

    let count: usize = REDIS
        .query(|mut q| async move {
            let count: usize = Script::new(
                r#"
                    local current
                    current = redis.call("incr",KEYS[1])
                    if current == 1 then
                        redis.call("expire",KEYS[1],ARGV[1])
                    end
                    return current
                "#,
            )
            .key(&key)
            .arg(Duration::minutes(5).num_seconds())
            .invoke_async(q.deref_mut())
            .await?;
            Ok(count)
        })
        .await?;

    Ok(count)
}

/// Generates a series of "incorrect" captcha answers, pushing them as InlineKeyboardButton
/// onto a Vec of buttons
fn insert_incorrect(
    ctx: &Context,
    res: &mut Vec<InlineKeyboardButton>,
    correct: &str,
    supported: &Vec<char>,
    rng: &mut ThreadRng,
    unmute_chat: i64,
) {
    let mut s = String::with_capacity(correct.len());
    for _ in correct.chars() {
        if let Some(ch) = supported.choose(rng) {
            s.push(*ch);
        }
    }
    let s = InlineKeyboardButtonBuilder::new(s)
        .set_callback_data(Uuid::new_v4().to_string())
        .build();
    let ctx = ctx.clone();
    s.on_push_multi(move |callback| {
        let ctx = ctx.clone();
        async move {
            if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
                let count = 3 - incorrect_tries(&callback, unmute_chat).await?;
                if count > 0 {
                    TG.client
                        .build_answer_callback_query(callback.get_id())
                        .show_alert(true)
                        .text(&lang_fmt!(ctx, "incorrect", count))
                        .build()
                        .await?;
                    Ok(false)
                } else {
                    TG.client
                        .build_answer_callback_query(callback.get_id())
                        .show_alert(true)
                        .text(&lang_fmt!(ctx, "notries"))
                        .build()
                        .await?;
                    kick(callback.get_from().get_id(), unmute_chat).await?;
                    if let Some(chat) = unmute_chat.get_chat().await? {
                        message
                            .speak(lang_fmt!(ctx, "notrieskickchat", chat.name_humanreadable()))
                            .await?;
                    } else {
                        message.speak(lang_fmt!(ctx, "notrieskick")).await?;
                    }
                    TG.client
                        .build_delete_message(message.get_chat().get_id(), message.get_message_id())
                        .build()
                        .await?;
                    reset_incorrect_tries(&callback.get_from(), unmute_chat).await?;
                    Ok(true)
                }
            } else {
                Ok(true)
            }
        }
    });
    res.push(s);
}

async fn get_invite_link<'a>(chat: &'a Chat) -> Result<Option<String>> {
    let unmute_chat = TG.client().build_get_chat(chat.get_id()).build().await?;

    Ok(unmute_chat.get_invite_link().map(|v| v.to_owned()))
}

fn get_choices<'a>(
    correct: String,
    supported: &Vec<char>,
    times: usize,
    unmute_chat: Chat,
    ctx: &Context,
) -> Vec<InlineKeyboardButton> {
    let mut rng = thread_rng();
    let mut res = Vec::<InlineKeyboardButton>::with_capacity(times);
    let pos = rng.gen_range(0..=times);
    log::info!("selected captcha correct pos {}", pos);
    let incorrect_chat = unmute_chat.get_id();
    for _ in 0..pos {
        insert_incorrect(
            &ctx,
            &mut res,
            correct.as_str(),
            supported,
            &mut rng,
            incorrect_chat,
        );
    }

    let correct_button = InlineKeyboardButtonBuilder::new(correct.clone())
        .set_callback_data(Uuid::new_v4().to_string())
        .build();
    let c = ctx.clone();
    correct_button.on_push(move |callback| async move {
        if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
            if let Some(link) = get_invite_link(&unmute_chat).await? {
                let mut button = InlineKeyboardBuilder::default();

                button.button(
                    InlineKeyboardButtonBuilder::new(lang_fmt!(c, "backtochat"))
                        .set_url(link)
                        .build(),
                );

                let button = button.build();

                TG.client()
                    .build_edit_message_caption()
                    .caption(&lang_fmt!(c, "correctchoice"))
                    .message_id(message.get_message_id())
                    .chat_id(message.get_chat().get_id())
                    .reply_markup(&button)
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_edit_message_caption()
                    .caption(&lang_fmt!(c, "correctchoice"))
                    .message_id(message.get_message_id())
                    .chat_id(message.get_chat().get_id())
                    .build()
                    .await?;
            }
            c.authorize_user(callback.get_from().get_id(), &unmute_chat)
                .await?;
            reset_incorrect_tries(&callback.get_from(), unmute_chat.get_id()).await?;
        }
        TG.client()
            .build_answer_callback_query(&callback.get_id())
            .build()
            .await?;

        Ok(())
    });
    res.push(correct_button);

    for _ in (pos + 1)..times {
        insert_incorrect(
            &ctx,
            &mut res,
            correct.as_str(),
            supported,
            &mut rng,
            incorrect_chat,
        );
    }
    res
}

/// Sends a "text" captcha to the specified chat
pub async fn send_captcha<'a>(message: &Message, unmute_chat: Chat, ctx: &Context) -> Result<()> {
    let (correct, bytes, supported) = build_captcha_sync();
    let mut builder = InlineKeyboardBuilder::default();
    for (i, choice) in get_choices(correct, &supported, 9, unmute_chat, ctx)
        .into_iter()
        .enumerate()
    {
        builder.button(choice);

        if i % 3 == 2 {
            builder.newline();
        }
    }
    TG.client()
        .build_send_photo(
            message.get_chat().get_id(),
            botapi::gen_types::FileData::Bytes(bytes),
        )
        .caption(&lang_fmt!(ctx, "captchawarning"))
        .reply_parameters(&ReplyParametersBuilder::new(message.get_message_id()).build())
        .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
            builder.build(),
        ))
        .build()
        .await?;

    Ok(())
}

async fn button_captcha<'a>(
    ctx: &Context,
    upd: &ChatMemberUpdated,
    captcha: &captchastate::Model,
    welcome: Option<welcomes::Model>,
    entities: Vec<MessageEntity>,
    buttons: Option<InlineKeyboardBuilder>,
) -> Result<()> {
    let unmute_button = InlineKeyboardButtonBuilder::new(lang_fmt!(ctx, "pressme"))
        .set_callback_data(Uuid::new_v4().to_string())
        .build();
    let bctx = ctx.clone();
    unmute_button.on_push(|callback| async move {
        bctx.authorize_user(callback.get_from().get_id(), bctx.try_get()?.chat)
            .await?;
        if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
            message
                .speak(lang_fmt!(bctx, "userunmuted"))
                .await?
                .delete_after_time(Duration::minutes(5));
        }

        Ok(())
    });
    let mut button = InlineKeyboardBuilder::default();
    button.button(unmute_button);
    if let Some(welcome) = welcome {
        welcome_members(
            ctx,
            upd,
            welcome,
            entities,
            buttons,
            ctx.lang(),
            Some(captcha),
        )
        .await?;
    } else {
        let m = TG
            .client()
            .build_send_message(
                upd.get_chat().get_id(),
                "Push the button to unmute yourself",
            )
            .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(button.build()))
            .build()
            .await?;
        m.delete_after_time(Duration::minutes(5));
    }

    Ok(())
}

#[inline(always)]
pub(crate) fn get_captcha_auth_key(user: i64, chat: i64) -> String {
    format!("cak:{}:{}", user, chat)
}

async fn send_captcha_chooser(
    ctx: &Context,
    upd: &ChatMemberUpdated,
    catpcha: &captchastate::Model,
    welcome: Option<welcomes::Model>,
    entities: Vec<MessageEntity>,
    buttons: Option<InlineKeyboardBuilder>,
    lang: &Lang,
) -> Result<()> {
    let user = upd.get_from();
    let chat = upd.get_chat();
    let url = get_captcha_url(chat, user).await?;
    let mut button = InlineKeyboardBuilder::default();
    button.button(
        InlineKeyboardButtonBuilder::new(lang_fmt!(ctx, "captcha"))
            .set_url(url)
            .build(),
    );

    if let Some(welcome) = welcome {
        welcome_members(ctx, upd, welcome, entities, buttons, lang, Some(catpcha)).await?;
    } else {
        let nm = TG
            .client()
            .build_send_message(chat.get_id(), &lang_fmt!(ctx, "solvecaptcha"))
            .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(button.build()))
            .build()
            .await?;
        nm.delete_after_time(Duration::minutes(5));
    }

    Ok(())
}

impl Context {
    /// Retrieve the current chat's captcah config, None if the captcha is disabled
    pub async fn get_captcha_config(&self) -> Result<Option<captchastate::Model>> {
        if let UpdateExt::ChatMember(upd) = self.update() {
            Ok(get_captcha_config(upd).await?)
        } else {
            Ok(None)
        }
    }

    async fn check_members<'a>(
        &self,
        config: &captchastate::Model,
        welcome: Option<welcomes::Model>,
        entities: Vec<MessageEntity>,
        buttons: Option<InlineKeyboardBuilder>,
    ) -> Result<()> {
        if let Some(UserChanged::UserJoined(ref message)) = self.update().user_event() {
            let me = ME.get().unwrap();
            let user = message.get_from();
            if user.get_id() == me.get_id() || user.is_admin(message.get_chat()).await? {
                return Ok(());
            }
            let chat = message.get_chat();
            if !user_is_authorized(chat.get_id(), user.get_id()).await? {
                self.mute(user.get_id(), self.try_get()?.chat, None).await?;
                let key = get_captcha_auth_key(user.get_id(), chat.get_id());
                REDIS
                    .pipe(|q| {
                        q.set(&key, true)
                            .expire(&key, Duration::minutes(10).num_seconds() as usize)
                    })
                    .await?;
                if let Some(kicktime) = config.kick_time {
                    let chatid = chat.get_id();
                    let userid = user.get_id();
                    tokio::spawn(async move {
                        sleep(Duration::seconds(kicktime).to_std()?).await;

                        if !user_is_authorized(chatid, userid).await? {
                            kick(userid, chatid).await?;
                        }
                        Ok::<(), BotError>(())
                    });
                }
                match config.captcha_type {
                    CaptchaType::Text => {
                        send_captcha_chooser(
                            self,
                            message,
                            config,
                            welcome,
                            entities,
                            buttons,
                            self.lang(),
                        )
                        .await?
                    }
                    CaptchaType::Button => {
                        button_captcha(self, message, config, welcome, entities, buttons).await?
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_welcome(
        &self,
        welcome: welcomes::Model,
        entities: Vec<MessageEntity>,
        goodbye: Vec<MessageEntity>,
        buttons: Option<InlineKeyboardBuilder>,
        gb_buttons: Option<InlineKeyboardBuilder>,
        captcha: Option<&captchastate::Model>,
    ) -> Result<()> {
        log::info!(
            "handle_welcome\n entities {:?}\ngoodbyes {:?}",
            entities,
            buttons.is_some()
        );
        if let Some(userchanged) = self.update().user_event() {
            if welcome.enabled {
                match userchanged {
                    UserChanged::UserJoined(member) => {
                        welcome_members(
                            self,
                            member,
                            welcome,
                            entities,
                            buttons,
                            &self.lang(),
                            captcha,
                        )
                        .await?
                    }
                    UserChanged::UserLeft(_) => {
                        goodbye_members(self, welcome, goodbye, gb_buttons, &self.lang()).await?
                    }
                }
            }
        }
        Ok(())
    }

    /// Send a captcha, welcome, or both to a user entering a chat
    pub async fn greeter_handle_update(&self) -> Result<()> {
        if let UpdateExt::ChatMember(ref upd) = self.update() {
            match (
                self.should_welcome(upd).await?,
                self.get_captcha_config().await?,
            ) {
                (Some((welcome, entities, goodbyes, buttons, gb_buttons)), None) => {
                    self.handle_welcome(welcome, entities, goodbyes, buttons, gb_buttons, None)
                        .await
                }
                (None, Some(captcha)) => self.check_members(&captcha, None, vec![], None).await,
                (Some((welcome, entities, _, buttons, _)), Some(captcha)) => {
                    self.check_members(&captcha, Some(welcome), entities, buttons)
                        .await
                }
                (None, None) => Ok(()),
            }?;
        }

        Ok(())
    }

    /// Enables captcha authentication for the current chat
    pub async fn enable_captcha(&self) -> Result<()> {
        let message = self.message()?;
        self.check_permissions(|p| p.can_change_info).await?;
        let model = captchastate::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            captcha_type: NotSet,
            kick_time: NotSet,
            captcha_text: NotSet,
        };
        let model = captchastate::Entity::insert(model)
            .on_conflict(
                OnConflict::column(captchastate::Column::Chat)
                    .update_column(captchastate::Column::Chat)
                    .to_owned(),
            )
            .exec_with_returning(DB.deref())
            .await?;
        let key = captcha_state_key(message.get_chat());
        model.cache(key).await?;
        message.reply("enabled captcha!").await?;
        Ok(())
    }

    /// Disabled captcha authenticate for the current chat
    pub async fn disable_captcha(&self) -> Result<()> {
        let message = self.message()?;
        self.check_permissions(|p| p.can_change_info).await?;
        let key = captcha_state_key(message.get_chat());
        captchastate::Entity::delete_by_id(message.get_chat().get_id())
            .exec(DB.deref())
            .await?;

        REDIS.sq(|q| q.del(&key)).await?;
        message.reply(lang_fmt!(self, "disabledcaptcha")).await?;
        Ok(())
    }

    /// Sets the number of seconds before a user who hasn't completed the captcha is
    /// removed from the chat. None to disable
    pub async fn captchakick(&self, kick: Option<i64>) -> Result<()> {
        let message = self.message()?;
        self.check_permissions(|p| p.can_change_info.and(p.can_restrict_members))
            .await?;
        let model = captchastate::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            captcha_type: NotSet,
            kick_time: Set(kick),
            captcha_text: NotSet,
        };

        let key = captcha_state_key(message.get_chat());
        if let Ok(model) = captchastate::Entity::update(model).exec(DB.deref()).await {
            model.cache(key).await?;
        }
        Ok(())
    }

    /// Sets the captcha type for the current chat
    pub async fn captchamode(&self, mode: CaptchaType) -> Result<()> {
        let message = self.message()?;
        self.check_permissions(|p| p.can_change_info).await?;
        let model = captchastate::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            captcha_type: Set(mode),
            kick_time: NotSet,
            captcha_text: NotSet,
        };

        let key = captcha_state_key(message.get_chat());
        if let Ok(model) = captchastate::Entity::update(model).exec(DB.deref()).await {
            log::info!("set captcha mode {:?}", model.captcha_type);
            let name = model.captcha_type.get_name();
            model.cache(key).await?;
            message.reply(lang_fmt!(self, "captchamode", name)).await?;
        } else {
            message.reply(lang_fmt!(self, "captchanotenabled")).await?;
        }
        Ok(())
    }

    async fn should_welcome(
        &self,
        upd: &ChatMemberUpdated,
    ) -> Result<
        Option<(
            welcomes::Model,
            Vec<MessageEntity>,
            Vec<MessageEntity>,
            Option<InlineKeyboardBuilder>,
            Option<InlineKeyboardBuilder>,
        )>,
    > {
        let chat = upd.get_chat();
        let key = format!("welcome:{}", chat.get_id());
        let chat_id = chat.get_id();

        let v: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
        let res = if let Some(v) = v {
            Ok(v.get()?)
        } else {
            let res = welcomes::get_filters_join(welcomes::Column::Chat.eq(chat_id)).await?;
            let res = res
                .into_iter()
                .map(|(model, (entity, goodbye, button, gb_button))| {
                    (
                        model,
                        entity
                            .into_iter()
                            .map(|e| e.get())
                            .map(|(e, u)| e.to_entity(u))
                            .collect(),
                        goodbye
                            .into_iter()
                            .map(|e| e.get())
                            .map(|(e, u)| e.to_entity(u))
                            .collect(),
                        get_markup_for_buttons(button.into_iter().collect()),
                        get_markup_for_buttons(gb_button.into_iter().collect()),
                    )
                })
                .next();

            if let Some(ref map) = res {
                REDIS
                    .try_pipe(|p| {
                        Ok(p.set(&key, map.to_redis()?)
                            .expire(&key, CONFIG.timing.cache_timeout))
                    })
                    .await?;
            }
            Ok(res)
        };
        res
    }

    /// Adds a user to the list of users that have completed the captcha for the current chat.
    /// These users will not be asked to complete the captcha again
    pub async fn authorize_user<'a>(&self, user: i64, unmute_chat: &Chat) -> Result<()> {
        let key = auth_key(unmute_chat.get_id());
        let (r, _): (i64, ()) = REDIS
            .pipe(|q| q.sadd(&key, user).expire(&key, CONFIG.timing.cache_timeout))
            .await?;
        if r == 1 {
            let model = authorized::Model {
                chat: unmute_chat.get_id(),
                user,
            };

            self.unmute(user, unmute_chat).await?;
            authorized::Entity::insert(model.into_active_model())
                .on_conflict(OnConflict::new().do_nothing().to_owned())
                .exec(DB.deref())
                .await?;
        }

        Ok(())
    }
}
