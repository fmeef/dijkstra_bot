use std::ops::DerefMut;

use self::entities::authorized;
use self::entities::captchastate::{self, CaptchaType};
use crate::metadata::metadata;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache, RedisStr};
use crate::statics::{CONFIG, DB, ME, REDIS, TG};
use crate::tg::admin_helpers::{kick, parse_duration, DeleteAfterTime, UpdateHelpers, UserChanged};
use crate::tg::button::{get_url, InlineKeyboardBuilder, OnPush};
use crate::tg::command::{ArgSlice, Context, TextArgs};
use crate::tg::permissions::*;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::util::error::Result;
use crate::util::string::Speak;
use base64::engine::general_purpose;
use base64::Engine;
use botapi::gen_types::{
    CallbackQuery, Chat, ChatMemberUpdated, EReplyMarkup, InlineKeyboardButton,
    InlineKeyboardButtonBuilder, Message, User,
};
use captcha::gen;
use chrono::Duration;
use lazy_static::__Deref;
use rand::rngs::ThreadRng;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use redis::{AsyncCommands, Script};
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
use sea_orm_migration::{MigrationName, MigrationTrait};
use tokio::time::sleep;
use uuid::Uuid;

metadata!("Captcha",
    r#"
       Set a captcha in the group to keep bots out. Supports two security levels, text and button. 
    "#,
    { command = "captcha", help = "Enabled or disables captcha. Usage: /captcha \\<on/off\\>" },
    { command = "captchamode", help = "Sets the captcha mode to either button or text"},
    { command = "captchakick", help = "Sets the timeout for removing users who haven't solved the captcha. off to disable"}

);

pub struct Migration;

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;
    use chrono::Duration;

    use self::captchastate::CaptchaType;

    use super::Migration;

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(captchastate::Entity)
                        .col(
                            ColumnDef::new(captchastate::Column::Chat)
                                .big_integer()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(captchastate::Column::CaptchaType)
                                .integer()
                                .not_null()
                                .default(CaptchaType::Button),
                        )
                        .col(
                            ColumnDef::new(captchastate::Column::KickTime)
                                .big_integer()
                                .default(Duration::minutes(1).num_seconds()),
                        )
                        .col(ColumnDef::new(captchastate::Column::CaptchaText).text())
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(authorized::Entity)
                        .col(
                            ColumnDef::new(authorized::Column::Chat)
                                .big_integer()
                                .not_null(),
                        )
                        .col(
                            ColumnDef::new(authorized::Column::User)
                                .big_integer()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(authorized::Column::Chat)
                                .col(authorized::Column::User)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager.drop_table_auto(captchastate::Entity).await?;
            manager.drop_table_auto(authorized::Entity).await?;
            Ok(())
        }
    }

    pub mod authorized {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "captcha_auth")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(primary_key)]
            pub user: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::captchastate::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
    pub mod captchastate {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        use crate::util::error::BotError;

        #[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug)]
        #[sea_orm(rs_type = "i32", db_type = "Integer")]
        pub enum CaptchaType {
            #[sea_orm(num_value = 1)]
            Button,
            #[sea_orm(num_value = 2)]
            Text,
        }

        impl CaptchaType {
            pub fn from_str(text: &str, chat: i64) -> crate::util::error::Result<Self> {
                match text {
                    "button" => Ok(CaptchaType::Button),
                    "text" => Ok(CaptchaType::Text),
                    _ => Err(BotError::speak("Invalid button type", chat)),
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "captcha")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(default = CaptchaType::Button)]
            pub captcha_type: CaptchaType,
            pub kick_time: Option<i64>,
            pub captcha_text: Option<String>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::captchastate::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230214_000001_create_captcha"
    }
}

fn captcha_state_key(chat: &Chat) -> String {
    format!("cstate:{}", chat.get_id())
}

async fn enable_captcha(message: &Message) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
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
    let key = captcha_state_key(message.get_chat_ref());
    model.cache(key).await?;
    message.reply("enabled captcha!").await?;
    Ok(())
}

async fn disable_captcha(message: &Message) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let key = captcha_state_key(message.get_chat_ref());
    captchastate::Entity::delete_by_id(message.get_chat().get_id())
        .exec(DB.deref())
        .await?;

    REDIS.sq(|q| q.del(&key)).await?;
    message.reply("disabled captcha").await?;
    Ok(())
}

fn auth_key(chat: i64) -> String {
    format!("cauth:{}", chat)
}

async fn update_auth_cache(chat: i64) -> Result<()> {
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

async fn user_is_authorized(chat: i64, user: i64) -> Result<bool> {
    update_auth_cache(chat).await?;
    let key = auth_key(chat);
    REDIS.sq(|q| q.sismember(&key, user)).await
}

async fn authorize_user<'a>(user: &User, ctx: &Context) -> Result<()> {
    if let Some(unmute_chat) = ctx.chat() {
        let key = auth_key(unmute_chat.get_id());
        let (r, _): (i64, ()) = REDIS
            .pipe(|q| {
                q.sadd(&key, user.get_id())
                    .expire(&key, CONFIG.timing.cache_timeout)
            })
            .await?;
        if r == 1 {
            let model = authorized::Model {
                chat: unmute_chat.get_id(),
                user: user.get_id(),
            };
            authorized::Entity::insert(model.into_active_model())
                .on_conflict(OnConflict::new().do_nothing().to_owned())
                .exec(DB.deref())
                .await?;
            ctx.unmute(user.get_id()).await?;
        }
    }
    Ok(())
}

async fn get_captcha_config(message: &ChatMemberUpdated) -> Result<Option<captchastate::Model>> {
    let key = captcha_state_key(message.get_chat_ref());
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

async fn captchamode(message: &Message, mode: CaptchaType) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let model = captchastate::ActiveModel {
        chat: Set(message.get_chat().get_id()),
        captcha_type: Set(mode),
        kick_time: NotSet,
        captcha_text: NotSet,
    };

    let key = captcha_state_key(message.get_chat_ref());
    if let Ok(model) = captchastate::Entity::update(model).exec(DB.deref()).await {
        model.cache(key).await?;
        message.reply("mode set").await?;
    } else {
        message.reply("captcha is not enabled").await?;
    }
    Ok(())
}

async fn captchakick_cmd<'a>(message: &Message, args: &'a TextArgs<'a>) -> Result<()> {
    message
        .check_permissions(|p| p.can_change_info.and(p.can_restrict_members))
        .await?;
    match args.as_slice() {
        ArgSlice { text: "off", .. } => {
            captchakick(message, None).await?;
            message.reply("Disabled captcha kick").await?;
        }
        slice => {
            if let Some(time) = parse_duration(&Some(slice), message.get_chat().get_id())? {
                captchakick(message, Some(time.num_seconds())).await?;
                message.reply("Enabled captcha kick").await?;
            } else {
                message.reply("Invalid argument").await?;
            }
        }
    }
    Ok(())
}

async fn captchakick(message: &Message, kick: Option<i64>) -> Result<()> {
    message
        .check_permissions(|p| p.can_change_info.and(p.can_restrict_members))
        .await?;
    let model = captchastate::ActiveModel {
        chat: Set(message.get_chat().get_id()),
        captcha_type: NotSet,
        kick_time: Set(kick),
        captcha_text: NotSet,
    };

    let key = captcha_state_key(message.get_chat_ref());
    if let Ok(model) = captchastate::Entity::update(model).exec(DB.deref()).await {
        model.cache(key).await?;
    }
    Ok(())
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

fn get_callback_key(key: &str) -> String {
    format!("ccback:{}", key)
}

pub async fn get_captcha_url(chat: &Chat, user: &User) -> Result<String> {
    let ser = RedisStr::new(&(chat, user))?;
    let r = Uuid::new_v4();
    let key = get_callback_key(&r.to_string());
    REDIS
        .pipe(|q| q.set(&key, ser).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    let bs = general_purpose::URL_SAFE_NO_PAD.encode(r.into_bytes());
    let bs = get_url(bs)?;
    Ok(bs)
}

async fn button_captcha<'a>(ctx: &'a Context) -> Result<()> {
    let unmute_button = InlineKeyboardButtonBuilder::new("Press me to unmute".to_owned())
        .set_callback_data(Uuid::new_v4().to_string())
        .build();
    let bctx = ctx.clone();
    unmute_button.on_push(|callback| async move {
        authorize_user(callback.get_from_ref(), &bctx).await?;
        if let Some(message) = callback.get_message() {
            message
                .speak("User unmuted!")
                .await?
                .delete_after_time(Duration::minutes(5));
        }

        Ok(())
    });
    let mut button = InlineKeyboardBuilder::default();
    button.button(unmute_button);
    if let Some(chat) = ctx.chat() {
        let m = TG
            .client()
            .build_send_message(chat.get_id(), "Push the button to unmute yourself")
            .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(button.build()))
            .build()
            .await?;
        m.delete_after_time(Duration::minutes(5));
    }
    Ok(())
}

async fn send_captcha_chooser(user: &User, chat: &Chat) -> Result<()> {
    let url = get_captcha_url(chat, user).await?;
    let mut button = InlineKeyboardBuilder::default();
    button.button(
        InlineKeyboardButtonBuilder::new("Captcha".to_owned())
            .set_url(url)
            .build(),
    );

    let nm = TG
        .client()
        .build_send_message(chat.get_id(), "Solve this captcha to continue")
        .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(button.build()))
        .build()
        .await?;
    nm.delete_after_time(Duration::minutes(5));

    Ok(())
}

#[inline(always)]
fn get_incorrect_counter(callback: &User, incorrect_chat: i64) -> String {
    format!("incc:{}:{}", callback.get_id(), incorrect_chat)
}

async fn reset_incorrect_tries(user: &User, chat: i64) -> Result<()> {
    let key = get_incorrect_counter(user, chat);
    REDIS.sq(|q| q.del(&key)).await
}

async fn incorrect_tries(callback: &CallbackQuery, incorrect_chat: i64) -> Result<usize> {
    let key = get_incorrect_counter(callback.get_from_ref(), incorrect_chat);

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

fn insert_incorrect(
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
    s.on_push_multi(move |callback| async move {
        if let Some(message) = callback.get_message() {
            let count = 3 - incorrect_tries(&callback, unmute_chat).await?;
            if count > 0 {
                TG.client
                    .build_answer_callback_query(callback.get_id_ref())
                    .show_alert(true)
                    .text(&format!("Incorect, tries remaining {}", count))
                    .build()
                    .await?;
                Ok(false)
            } else {
                TG.client
                    .build_answer_callback_query(callback.get_id_ref())
                    .show_alert(true)
                    .text("No tries remaining")
                    .build()
                    .await?;
                kick(callback.get_from().get_id(), unmute_chat).await?;
                message
                    .speak("No tries remaining, you have been kicked from the chat")
                    .await?;
                TG.client
                    .build_delete_message(message.get_chat().get_id(), message.get_message_id())
                    .build()
                    .await?;
                reset_incorrect_tries(&callback.get_from(), unmute_chat).await?;
                Ok(true)
            }
        } else {
            log::warn!("message not found");
            Ok(true)
        }
    });
    res.push(s);
}

async fn get_invite_link<'a>(chat: &'a Chat) -> Result<Option<String>> {
    let unmute_chat = TG.client().build_get_chat(chat.get_id()).build().await?;

    Ok(unmute_chat.get_invite_link().map(|v| v.into_owned()))
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
    let pos = rng.gen_range(0..times);
    let incorrect_chat = unmute_chat.get_id();
    for _ in 0..pos {
        insert_incorrect(
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
    let ctx = ctx.clone();
    correct_button.on_push(move |callback| async move {
        if let Some(message) = callback.get_message() {
            if let Some(link) = get_invite_link(&unmute_chat).await? {
                let mut button = InlineKeyboardBuilder::default();

                button.button(
                    InlineKeyboardButtonBuilder::new("Back to chat".to_owned())
                        .set_url(link)
                        .build(),
                );

                let button = button.build();

                TG.client()
                    .build_edit_message_caption()
                    .caption("Correct choice!")
                    .message_id(message.get_message_id())
                    .chat_id(message.get_chat().get_id())
                    .reply_markup(&button)
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_edit_message_caption()
                    .caption("Correct choice!")
                    .message_id(message.get_message_id())
                    .chat_id(message.get_chat().get_id())
                    .build()
                    .await?;
            }
            authorize_user(&callback.get_from(), &ctx).await?;
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
            &mut res,
            correct.as_str(),
            supported,
            &mut rng,
            incorrect_chat,
        );
    }
    res
}

fn build_captcha_sync() -> (String, Vec<u8>, Vec<char>) {
    let captcha = gen(captcha::Difficulty::Hard);

    (
        captcha.chars_as_string(),
        captcha.as_png().unwrap(),
        captcha.supported_chars(),
    )
}

async fn send_captcha<'a>(message: &Message, unmute_chat: Chat, ctx: &Context) -> Result<()> {
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
        .caption("If you do not solve this captcha correctly you will be terminated by memetic kill agent")
        .reply_to_message_id(message.get_message_id())
        .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
            builder.build(),
        ))
        .build()
        .await?;

    Ok(())
}

async fn check_mambers<'a>(
    message: &ChatMemberUpdated,
    ctx: &Context,
    config: &captchastate::Model,
) -> Result<()> {
    let me = ME.get().unwrap();
    let user = message.get_from();
    if user.get_id() == me.get_id() || user.is_admin(message.get_chat_ref()).await? {
        return Ok(());
    }
    let chat = message.get_chat();
    if !user_is_authorized(chat.get_id(), user.get_id()).await? {
        ctx.mute(user.get_id(), None).await?;
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
            CaptchaType::Text => send_captcha_chooser(&user, &chat).await?,
            CaptchaType::Button => button_captcha(ctx).await?,
        }
    }

    Ok(())
}
async fn handle_user_action<'a>(ctx: &Context, message: &ChatMemberUpdated) -> Result<()> {
    if let Some(config) = get_captcha_config(message).await? {
        check_mambers(message, ctx, &config).await?;
    }
    Ok(())
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some((cmd, _, args, message, _)) = ctx.cmd() {
        match cmd {
            "captchakick" => {
                captchakick_cmd(message, args).await?;
            }
            "captchamode" => {
                let t = CaptchaType::from_str(
                    args.args.first().map(|a| a.get_text()).unwrap_or(""),
                    message.get_chat().get_id(),
                )?;
                captchamode(message, t).await?;
            }
            "captcha" => match args.args.first().map(|a| a.get_text()) {
                Some("on") => enable_captcha(message).await?,
                Some("off") => disable_captcha(message).await?,
                _ => {
                    return Err(BotError::speak(
                        "Invalid argument, use on or off",
                        message.get_chat().get_id(),
                    ))
                }
            },
            "start" => {
                if let (Some(user), Some(u)) =
                    (message.get_from(), args.args.first().map(|a| a.get_text()))
                {
                    let base = general_purpose::URL_SAFE_NO_PAD.decode(u)?;
                    let base = Uuid::from_slice(base.as_slice())?;
                    let key = get_callback_key(&base.to_string());
                    let base: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
                    if let Some(base) = base {
                        let (cchat, cuser): (Chat, User) = base.get()?;
                        log::info!("chat {}", cchat.name_humanreadable());
                        if cuser.get_id() == user.get_id() {
                            send_captcha(message, cchat, ctx).await?;
                        }
                    }
                }
            }
            _ => (),
        };
    }
    Ok(())
}

pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    let update = &cmd.get_static().update;
    if let Some(UserChanged::UserJoined(ref member)) = update.user_event() {
        handle_user_action(cmd, member).await?;
    }

    handle_command(cmd).await?;

    Ok(())
}
