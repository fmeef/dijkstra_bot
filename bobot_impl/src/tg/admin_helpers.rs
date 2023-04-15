use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
};

use crate::{
    persist::{
        admin::{
            actions::{self, ActionType},
            warns,
        },
        core::dialogs,
        redis::{default_cache_query, CachedQuery, CachedQueryTrait, RedisCache, RedisStr},
    },
    statics::{CONFIG, DB, REDIS, TG},
    util::error::{BotError, Result},
    util::string::{get_chat_lang, Speak},
};
use async_trait::async_trait;
use botapi::gen_types::{
    Chat, ChatMember, ChatPermissions, ChatPermissionsBuilder, Message, UpdateExt, User,
};
use chrono::{DateTime, Duration, Utc};
use futures::{future::BoxFuture, FutureExt};

use lazy_static::__Deref;
use macros::{entity_fmt, lang_fmt};
use redis::AsyncCommands;

use sea_orm::{
    sea_query::OnConflict, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait,
    IntoActiveModel, ModelTrait, PaginatorTrait, QueryFilter,
};

use super::{
    command::{ArgSlice, Entities, EntityArg, TextArgs},
    dialog::{dialog_or_default, get_dialog_key},
    markdown::MarkupType,
    user::{get_me, get_user_username, GetUser, Username},
};

pub struct ChatUser<'a> {
    pub chat: Cow<'a, Chat>,
    pub user: Cow<'a, User>,
}

pub trait IntoChatUser {
    fn get_chatuser<'a>(&'a self) -> Option<ChatUser<'a>>;
    fn get_chatuser_user<'a>(&'a self, user: Cow<'a, User>) -> ChatUser<'a>;
}

pub trait DeleteAfterTime {
    fn delete_after_time(&self, duration: Duration);
}

impl DeleteAfterTime for Message {
    fn delete_after_time(&self, duration: Duration) {
        let chat_id = self.get_chat().get_id();
        let message_id = self.get_message_id();

        tokio::spawn(async move {
            tokio::time::sleep(duration.to_std()?).await;
            if let Err(err) = TG
                .client
                .build_delete_message(chat_id, message_id)
                .build()
                .await
            {
                BotError::from(err).record_stats();
            }

            Ok::<(), BotError>(())
        });
    }
}

impl DeleteAfterTime for Option<Message> {
    fn delete_after_time(&self, duration: Duration) {
        if let Some(message) = self {
            message.delete_after_time(duration);
        }
    }
}

impl IntoChatUser for Message {
    fn get_chatuser<'a>(&'a self) -> Option<ChatUser<'a>> {
        self.get_from_ref().map(|f| ChatUser {
            user: Cow::Borrowed(f),
            chat: self.get_chat(),
        })
    }

    fn get_chatuser_user<'a>(&'a self, user: Cow<'a, User>) -> ChatUser<'a> {
        ChatUser {
            user,
            chat: self.get_chat(),
        }
    }
}

pub async fn is_self_admin(chat: &Chat) -> Result<bool> {
    let me = get_me().await?;
    Ok(chat.is_user_admin(me.get_id()).await?.is_some())
}

pub fn is_dm(chat: &Chat) -> bool {
    chat.get_tg_type() == "private"
}

fn get_action_key(user: i64, chat: i64) -> String {
    format!("act:{}:{}", user, chat)
}

fn get_warns_key(user: i64, chat: i64) -> String {
    format!("warns:{}:{}", user, chat)
}

pub async fn kick(user: i64, chat: i64) -> Result<()> {
    TG.client()
        .build_ban_chat_member(chat, user)
        .build()
        .await?;
    TG.client()
        .build_unban_chat_member(chat, user)
        .build()
        .await?;
    Ok(())
}

pub async fn kick_message(message: &Message) -> Result<()> {
    if let Some(from) = message.get_from() {
        TG.client()
            .build_ban_chat_member(message.get_chat().get_id(), from.get_id())
            .build()
            .await?;
        TG.client()
            .build_unban_chat_member(message.get_chat().get_id(), from.get_id())
            .build()
            .await?;
    }
    Ok(())
}

pub async fn change_permissions(
    chat: &Chat,
    user: &User,
    permissions: &ChatPermissions,
    time: Option<Duration>,
) -> Result<()> {
    let me = get_me().await?;
    let lang = get_chat_lang(chat.get_id()).await?;
    if user.is_admin(chat).await? {
        Err(BotError::speak(lang_fmt!(lang, "muteadmin"), chat.get_id()))
    } else {
        if user.get_id() == me.get_id() {
            chat.speak(lang_fmt!(lang, "mutemyself")).await?;
            Err(BotError::speak(
                lang_fmt!(lang, "mutemyself"),
                chat.get_id(),
            ))
        } else {
            if let Some(time) = time.map(|t| Utc::now().checked_add_signed(t)).flatten() {
                TG.client()
                    .build_restrict_chat_member(chat.get_id(), user.get_id(), permissions)
                    .until_date(time.timestamp())
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_restrict_chat_member(chat.get_id(), user.get_id(), permissions)
                    .build()
                    .await?;
            }
            let time = time.map(|t| Utc::now().checked_add_signed(t)).flatten();
            update_actions_permissions(user, chat, permissions, time).await?;
            Ok(())
        }
    }
}

pub async fn action_message<'a, F>(
    message: &'a Message,
    entities: &Entities<'a>,
    args: Option<&'a TextArgs<'a>>,
    action: F,
) -> Result<User>
where
    for<'b> F: FnOnce(&'b Message, &'b User, Option<ArgSlice<'b>>) -> BoxFuture<'b, Result<()>>,
{
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    if let Some(user) = message
        .get_reply_to_message_ref()
        .map(|v| v.get_from())
        .flatten()
    {
        action(&message, &user, args.map(|a| a.as_slice())).await?;
        Ok(user.into_owned())
    } else {
        match entities.front() {
            Some(EntityArg::Mention(name)) => {
                if let Some(user) = get_user_username(name).await? {
                    action(message, &user, args.map(|a| a.pop_slice()).flatten()).await?;
                    Ok(user)
                } else {
                    return Err(BotError::speak(
                        lang_fmt!(lang, "usernotfound"),
                        message.get_chat().get_id(),
                    ));
                }
            }
            Some(EntityArg::TextMention(user)) => {
                action(message, user, args.map(|a| a.pop_slice()).flatten()).await?;
                Ok((*user).to_owned())
            }
            _ => {
                return Err(BotError::speak(
                    lang_fmt!(lang, "specifyuser"),
                    message.get_chat().get_id(),
                ));
            }
        }
    }
}

pub fn parse_duration_str(arg: &str, chat: i64) -> Result<Option<Duration>> {
    let head = &arg[0..arg.len() - 1];
    let tail = &arg[arg.len() - 1..];
    log::info!("head {} tail {}", head, tail);
    let head = match str::parse::<i64>(head) {
        Err(_) => return Err(BotError::speak("Enter a number", chat)),
        Ok(res) => res,
    };
    let res = match tail {
        "m" => Duration::minutes(head),
        "h" => Duration::hours(head),
        "d" => Duration::days(head),
        _ => return Err(BotError::speak("Invalid time spec", chat)),
    };

    let res = if res.num_seconds() < 30 {
        Duration::seconds(30)
    } else {
        res
    };

    Ok(Some(res))
}

pub fn parse_duration<'a>(args: &Option<ArgSlice<'a>>, chat: i64) -> Result<Option<Duration>> {
    if let Some(args) = args {
        if let Some(thing) = args.args.first() {
            let head = &thing.get_text()[0..thing.get_text().len() - 1];
            let tail = &thing.get_text()[thing.get_text().len() - 1..];
            log::info!("head {} tail {}", head, tail);
            let head = match str::parse::<i64>(head) {
                Err(_) => return Err(BotError::speak("Enter a number", chat)),
                Ok(res) => res,
            };
            let res = match tail {
                "m" => Duration::minutes(head),
                "h" => Duration::hours(head),
                "d" => Duration::days(head),
                _ => return Err(BotError::speak("Invalid time spec", chat)),
            };

            let res = if res.num_seconds() < 30 {
                Duration::seconds(30)
            } else {
                res
            };

            Ok(Some(res))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

pub async fn change_permissions_message<'a>(
    message: &Message,
    entities: &VecDeque<EntityArg<'a>>,
    permissions: ChatPermissions,
    args: &'a TextArgs<'a>,
) -> Result<User> {
    action_message(message, entities, Some(args), |message, user, args| {
        async move {
            let duration = parse_duration(&args, message.get_chat().get_id())?;
            change_permissions(message.get_chat_ref(), user, &permissions, duration).await?;

            Ok(())
        }
        .boxed()
    })
    .await
}

pub async fn warn_with_action(
    message: &Message,
    user: &User,
    reason: Option<&str>,
    duration: Option<Duration>,
) -> Result<(i32, i32)> {
    let dialog = dialog_or_default(message.get_chat_ref()).await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let time = dialog.warn_time.map(|t| Duration::seconds(t));
    let count = warn_user(message, user, reason.map(|v| v.to_owned()), &time).await?;

    let name = user.name_humanreadable();
    if let Some(reason) = reason {
        message
            .reply(lang_fmt!(
                lang,
                "warnreason",
                name,
                count,
                dialog.warn_limit,
                reason
            ))
            .await?;
    } else {
        message
            .reply(lang_fmt!(lang, "warn", name, count, dialog.warn_limit))
            .await?;
    }

    if count >= dialog.warn_limit {
        match dialog.action_type {
            actions::ActionType::Mute => warn_mute(message, user, count, duration).await,
            actions::ActionType::Ban => warn_ban(message, user, count, duration).await,
            actions::ActionType::Shame => warn_shame(message, user, count).await,
            actions::ActionType::Warn => Ok(()),
            actions::ActionType::Delete => Ok(()),
        }?;
    }
    Ok((count, dialog.warn_limit))
}

pub async fn set_warn_time(chat: &Chat, time: i64) -> Result<()> {
    let chat_id = chat.get_id();

    let model = dialogs::ActiveModel {
        chat_id: Set(chat_id),
        language: NotSet,
        chat_type: Set(chat.get_tg_type().into_owned()),
        warn_limit: NotSet,
        action_type: NotSet,
        warn_time: Set(Some(time)),
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::WarnTime)
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    model.cache(key).await?;
    Ok(())
}

pub async fn set_warn_limit(chat: &Chat, limit: i32) -> Result<()> {
    let chat_id = chat.get_id();

    let model = dialogs::ActiveModel {
        chat_id: Set(chat_id),
        language: NotSet,
        chat_type: Set(chat.get_tg_type().into_owned()),
        warn_limit: Set(limit),
        action_type: NotSet,
        warn_time: NotSet,
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::WarnLimit)
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    model.cache(key).await?;
    Ok(())
}

pub async fn set_warn_mode(chat: &Chat, mode: &str) -> Result<()> {
    let chat_id = chat.get_id();
    let mode = match mode {
        "mute" => Ok(ActionType::Mute),
        "ban" => Ok(ActionType::Ban),
        "shame" => Ok(ActionType::Shame),
        _ => Err(BotError::speak(format!("Invalid mode {}", mode), chat_id)),
    }?;

    let model = dialogs::ActiveModel {
        chat_id: Set(chat_id),
        language: NotSet,
        chat_type: Set(chat.get_tg_type().into_owned()),
        warn_limit: NotSet,
        action_type: Set(mode),
        warn_time: NotSet,
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::ActionType)
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    model.cache(key).await?;
    Ok(())
}
pub async fn get_action(chat: &Chat, user: &User) -> Result<Option<actions::Model>> {
    let chat = chat.get_id();
    let user = user.get_id();
    let key = get_action_key(user, chat);
    let res = default_cache_query(
        move |_, _| async move {
            let res = actions::Entity::find_by_id((user, chat))
                .one(DB.deref())
                .await?;
            Ok(res)
        },
        Duration::hours(1),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

pub async fn warn_ban(
    message: &Message,
    user: &User,
    count: i32,
    duration: Option<Duration>,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    ban(message, user, duration).await?;
    message
        .reply(&lang_fmt!(
            lang,
            "warnban",
            count,
            user.name_humanreadable()
        ))
        .await?;
    Ok(())
}

pub async fn warn_mute(
    message: &Message,
    user: &User,
    count: i32,
    duration: Option<Duration>,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    mute(message.get_chat_ref(), user, duration).await?;

    let name = user.name_humanreadable();
    let mention = MarkupType::TextMention(user.to_owned()).text(&name);
    message
        .reply_fmt(entity_fmt!(
            lang,
            message.get_chat().get_id(),
            "warnmute",
            MarkupType::Text.text(&count.to_string()),
            mention
        ))
        .await?;

    Ok(())
}

pub async fn warn_shame(message: &Message, _user: &User, _count: i32) -> Result<()> {
    message.speak("shaming not implemented").await?;

    Ok(())
}

pub async fn get_warns(message: &Message, user: &User) -> Result<Vec<warns::Model>> {
    let user_id = user.get_id();
    let chat_id = message.get_chat().get_id();
    let key = get_warns_key(user.get_id(), message.get_chat().get_id());
    let r = CachedQuery::new(
        |_, _| async move {
            let count = warns::Entity::find()
                .filter(
                    warns::Column::UserId
                        .eq(user_id)
                        .and(warns::Column::ChatId.eq(chat_id)),
                )
                .all(DB.deref().deref())
                .await?;
            Ok(count)
        },
        |key, _| async move {
            let count: Vec<RedisStr> = REDIS.sq(|q| q.smembers(&key)).await?;
            if count.len() > 0 {
                log::info!("miss! {}", count.len());
                Ok(Some(
                    count
                        .into_iter()
                        .filter_map(|v| v.get::<warns::Model>().ok())
                        .collect(),
                ))
            } else {
                Ok(None)
            }
        },
        |key, warns| async move {
            REDIS
                .try_pipe(|q| {
                    for v in &warns {
                        let ins = RedisStr::new(&v)?;
                        q.sadd(key, ins);
                    }
                    Ok(q.expire(key, CONFIG.timing.cache_timeout as usize))
                })
                .await?;
            Ok(warns)
        },
    )
    .query(&key, &())
    .await?;
    let mut res = Vec::<warns::Model>::new();
    for warn in r {
        if let Some(expire) = &warn.expires {
            if Utc::now().timestamp() > expire.timestamp() {
                log::info!("warn expired!");
                let args = RedisStr::new(&warn)?;
                REDIS.sq(|q| q.srem(&key, &args)).await?;
                warn.delete(DB.deref()).await?;
            } else {
                res.push(warn);
            }
        } else {
            res.push(warn);
        }
    }
    Ok(res)
}

pub async fn get_warns_count(message: &Message, user: &User) -> Result<i32> {
    let user_id = user.get_id();
    let chat_id = message.get_chat().get_id();
    let key = get_warns_key(user.get_id(), message.get_chat().get_id());
    let v: Option<i32> = REDIS.sq(|q| q.scard(&key)).await?;
    if let Some(v) = v {
        Ok(v)
    } else {
        let r = CachedQuery::new(
            |_, _| async move {
                let count = warns::Entity::find()
                    .filter(
                        warns::Column::UserId
                            .eq(user_id)
                            .and(warns::Column::ChatId.eq(chat_id)),
                    )
                    .count(DB.deref().deref())
                    .await?;
                Ok(count)
            },
            |key, _| async move {
                let count: Option<u64> = REDIS.sq(|q| q.llen(&key)).await?;
                Ok(count)
            },
            |_, v| async move { Ok(v) },
        )
        .query(&key, &())
        .await?;
        Ok(r as i32)
    }
}

pub async fn clear_warns(chat: &Chat, user: &User) -> Result<()> {
    let key = get_warns_key(user.get_id(), chat.get_id());
    REDIS.sq(|q| q.del(&key)).await?;
    warns::Entity::delete_many()
        .filter(
            warns::Column::ChatId
                .eq(chat.get_id())
                .and(warns::Column::UserId.eq(user.get_id())),
        )
        .exec(DB.deref().deref())
        .await?;
    Ok(())
}

pub async fn unmute(chat: &Chat, user: &User) -> Result<()> {
    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(true)
        .set_can_send_audios(true)
        .set_can_send_documents(true)
        .set_can_send_photos(true)
        .set_can_send_videos(true)
        .set_can_send_video_notes(true)
        .set_can_send_polls(true)
        .set_can_send_voice_notes(true)
        .set_can_send_other_messages(true)
        .build();

    change_permissions(chat, user, &permissions, None).await?;
    Ok(())
}

pub async fn mute(chat: &Chat, user: &User, duration: Option<Duration>) -> Result<()> {
    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(false)
        .set_can_send_audios(false)
        .set_can_send_documents(false)
        .set_can_send_photos(false)
        .set_can_send_videos(false)
        .set_can_send_video_notes(false)
        .set_can_send_polls(false)
        .set_can_send_voice_notes(false)
        .set_can_send_other_messages(false)
        .build();

    change_permissions(chat, user, &permissions, duration).await?;
    Ok(())
}

pub async fn unban(message: &Message, user: &User) -> Result<()> {
    if let Some(senderchat) = message.get_sender_chat() {
        TG.client()
            .build_unban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
            .build()
            .await?;
    } else {
        TG.client()
            .build_unban_chat_member(message.get_chat().get_id(), user.get_id())
            .build()
            .await?;
    }
    Ok(())
}

pub async fn ban_message(message: &Message, duration: Option<Duration>) -> Result<()> {
    if let Some(senderchat) = message.get_sender_chat() {
        TG.client()
            .build_ban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
            .build()
            .await?;
    } else {
        if let Some(user) = message.get_from() {
            if let Some(duration) = duration.map(|v| Utc::now().checked_add_signed(v)).flatten() {
                TG.client()
                    .build_ban_chat_member(message.get_chat().get_id(), user.get_id())
                    .until_date(duration.timestamp())
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_ban_chat_member(message.get_chat().get_id(), user.get_id())
                    .build()
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn ban(message: &Message, user: &User, duration: Option<Duration>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    if let Some(senderchat) = message.get_sender_chat() {
        TG.client()
            .build_ban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
            .build()
            .await?;
        let name = senderchat.name_humanreadable();

        let mention = MarkupType::TextMention(user.to_owned()).text(&name);
        message
            .speak_fmt(entity_fmt!(
                lang,
                message.get_chat().get_id(),
                "banchat",
                mention
            ))
            .await?;
    } else {
        if let Some(duration) = duration.map(|v| Utc::now().checked_add_signed(v)).flatten() {
            TG.client()
                .build_ban_chat_member(message.get_chat().get_id(), user.get_id())
                .until_date(duration.timestamp())
                .build()
                .await?;
        } else {
            TG.client()
                .build_ban_chat_member(message.get_chat().get_id(), user.get_id())
                .build()
                .await?;
        }

        let name = user.name_humanreadable();

        let mention = MarkupType::TextMention(user.to_owned()).text(&name);
        message
            .speak_fmt(entity_fmt!(
                lang,
                message.get_chat().get_id(),
                "banned",
                mention
            ))
            .await?;
    }
    Ok(())
}

pub async fn warn_user(
    message: &Message,
    user: &User,
    reason: Option<String>,
    duration: &Option<Duration>,
) -> Result<i32> {
    let user_id = user.get_id();
    let chat_id = message.get_chat().get_id();
    let duration = duration.map(|v| Utc::now().checked_add_signed(v)).flatten();
    let model = warns::ActiveModel {
        id: NotSet,
        user_id: Set(user_id),
        chat_id: Set(chat_id),
        reason: Set(reason),
        expires: Set(duration),
    };
    let model = warns::Entity::insert(model)
        .exec_with_returning(DB.deref().deref())
        .await?;
    let model = RedisStr::new(&model)?;
    let key = get_warns_key(user_id, chat_id);
    let (_, _, count): ((), (), usize) = REDIS
        .pipe(|p| {
            p.sadd(&key, model)
                .expire(&key, CONFIG.timing.cache_timeout)
                .scard(&key)
        })
        .await?;

    Ok(count as i32)
}

pub async fn update_actions_ban(
    chat: &Chat,
    user: &User,
    banned: bool,
    expires: Option<DateTime<Utc>>,
) -> Result<()> {
    let key = get_action_key(user.get_id(), chat.get_id());

    let active = actions::ActiveModel {
        user_id: Set(user.get_id()),
        chat_id: Set(chat.get_id()),
        pending: Set(true),
        is_banned: Set(banned),
        can_send_messages: NotSet,
        can_send_audio: NotSet,
        can_send_video: NotSet,
        can_send_photo: NotSet,
        can_send_document: NotSet,
        can_send_voice_note: NotSet,
        can_send_video_note: NotSet,
        can_send_poll: NotSet,
        can_send_other: NotSet,
        action: NotSet,
        expires: Set(expires),
    };

    let res = actions::Entity::insert(active)
        .on_conflict(
            OnConflict::columns([actions::Column::UserId, actions::Column::ChatId])
                .update_columns([actions::Column::IsBanned, actions::Column::Expires])
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    res.cache(key).await?;
    Ok(())
}

pub async fn update_actions_pending(chat: &Chat, user: &User, pending: bool) -> Result<()> {
    let key = get_action_key(user.get_id(), chat.get_id());

    let active = actions::ActiveModel {
        user_id: Set(user.get_id()),
        chat_id: Set(chat.get_id()),
        pending: Set(pending),
        is_banned: NotSet,
        can_send_messages: NotSet,
        can_send_audio: NotSet,
        can_send_video: NotSet,
        can_send_photo: NotSet,
        can_send_document: NotSet,
        can_send_voice_note: NotSet,
        can_send_video_note: NotSet,
        can_send_poll: NotSet,
        can_send_other: NotSet,
        action: NotSet,
        expires: NotSet,
    };

    let res = actions::Entity::insert(active)
        .on_conflict(
            OnConflict::columns([actions::Column::UserId, actions::Column::ChatId])
                .update_columns([actions::Column::Pending])
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    res.cache(key).await?;

    Ok(())
}

pub async fn update_actions_permissions(
    user: &User,
    chat: &Chat,
    permissions: &ChatPermissions,
    expires: Option<DateTime<Utc>>,
) -> Result<()> {
    let key = get_action_key(user.get_id(), chat.get_id());

    let active = actions::ActiveModel {
        user_id: Set(user.get_id()),
        chat_id: Set(chat.get_id()),
        pending: Set(true),
        is_banned: NotSet,
        can_send_messages: permissions
            .get_can_send_messages()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_audio: permissions
            .get_can_send_audios()
            .map(|v| Set(v))
            .unwrap_or(NotSet),

        can_send_document: permissions
            .get_can_send_documents()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_photo: permissions
            .get_can_send_photos()
            .map(|v| Set(v))
            .unwrap_or(NotSet),

        can_send_video: permissions
            .get_can_send_videos()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_voice_note: permissions
            .get_can_send_voice_notes()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_video_note: permissions
            .get_can_send_video_notes()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_poll: permissions
            .get_can_send_polls()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        can_send_other: permissions
            .get_can_send_other_messages()
            .map(|v| Set(v))
            .unwrap_or(NotSet),
        action: NotSet,
        expires: Set(expires),
    };

    let res = actions::Entity::insert(active)
        .on_conflict(
            OnConflict::columns([actions::Column::UserId, actions::Column::ChatId])
                .update_columns([
                    actions::Column::Pending,
                    actions::Column::CanSendMessages,
                    actions::Column::CanSendAudio,
                    actions::Column::CanSendVideo,
                    actions::Column::CanSendDocument,
                    actions::Column::CanSendPhoto,
                    actions::Column::CanSendVoiceNote,
                    actions::Column::CanSendVideoNote,
                    actions::Column::CanSendPoll,
                    actions::Column::CanSendOther,
                    actions::Column::Expires,
                ])
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    res.cache(key).await?;

    Ok(())
}

pub async fn handle_pending_action(update: &UpdateExt) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => {
            if !is_dm(&message.get_chat()) {
                if let Some(user) = message.get_from_ref() {
                    handle_pending_action_user(user, message.get_chat_ref()).await?;
                }
            }
        }
        _ => (),
    };

    Ok(())
}

pub async fn handle_pending_action_user(user: &User, chat: &Chat) -> Result<()> {
    if !is_self_admin(&chat).await? {
        return Ok(());
    }
    if let Some(action) = get_action(&chat, &user).await? {
        log::info!("handling pending action user {}", user.name_humanreadable());
        let time = Utc::now();
        if let Some(expire) = action.expires {
            if expire < time {
                log::info!("expired action!");
                if action.is_banned {
                    TG.client()
                        .build_unban_chat_member(chat.get_id(), user.get_id())
                        .build()
                        .await?;
                }

                unmute(&chat, &user).await?;
                action.delete(DB.deref()).await?;
                return Ok(());
            }
        }
        if action.pending {
            let lang = get_chat_lang(chat.get_id()).await?;

            let name = user.name_humanreadable();
            if action.is_banned {
                TG.client()
                    .build_ban_chat_member(chat.get_id(), user.get_id())
                    .build()
                    .await?;

                let mention = MarkupType::TextMention(user.to_owned()).text(&name);
                chat.speak_fmt(entity_fmt!(lang, chat.get_id(), "banned", mention))
                    .await?;
            } else {
                let permissions = ChatPermissionsBuilder::new()
                    .set_can_send_messages(action.can_send_messages)
                    .set_can_send_polls(action.can_send_poll)
                    .set_can_send_other_messages(action.can_send_other)
                    .set_can_send_audios(action.can_send_audio)
                    .set_can_send_documents(action.can_send_document)
                    .set_can_send_photos(action.can_send_photo)
                    .set_can_send_videos(action.can_send_video)
                    .set_can_send_video_notes(action.can_send_video_note)
                    .set_can_send_voice_notes(action.can_send_voice_note)
                    .build();
                TG.client()
                    .build_restrict_chat_member(chat.get_id(), user.get_id(), &permissions)
                    .build()
                    .await?;
            }

            update_actions_pending(&chat, &user, false).await?;
        }
    }

    Ok(())
}

pub async fn update_actions(actions: actions::Model) -> Result<()> {
    let key = get_action_key(actions.user_id, actions.chat_id);

    actions::Entity::insert(actions.cache(key).await?.into_active_model())
        .on_conflict(
            OnConflict::columns([actions::Column::UserId, actions::Column::ChatId])
                .update_columns([
                    actions::Column::IsBanned,
                    actions::Column::CanSendMessages,
                    actions::Column::Action,
                    actions::Column::CanSendAudio,
                    actions::Column::CanSendVideo,
                    actions::Column::CanSendDocument,
                    actions::Column::CanSendPhoto,
                    actions::Column::CanSendVoiceNote,
                    actions::Column::CanSendVideoNote,
                    actions::Column::CanSendPoll,
                    actions::Column::CanSendOther,
                ])
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;
    Ok(())
}

pub async fn is_dm_or_die(chat: &Chat) -> Result<()> {
    let lang = get_chat_lang(chat.get_id()).await?;
    if !is_dm(chat) {
        Err(BotError::speak(lang_fmt!(lang, "notdm"), chat.get_id()))
    } else {
        Ok(())
    }
}

pub async fn is_group_or_die(chat: &Chat) -> Result<()> {
    let lang = get_chat_lang(chat.get_id()).await?;
    match chat.get_tg_type().as_ref() {
        "private" => Err(BotError::speak(lang_fmt!(lang, "baddm"), chat.get_id())),
        "group" => Err(BotError::speak(
            lang_fmt!(lang, "notsupergroup"),
            chat.get_id(),
        )),
        _ => Ok(()),
    }
}

pub async fn self_admin_or_die(chat: &Chat) -> Result<()> {
    if !is_self_admin(chat).await? {
        let lang = get_chat_lang(chat.get_id()).await?;
        Err(BotError::speak(
            lang_fmt!(lang, "needtobeadmin"),
            chat.get_id(),
        ))
    } else {
        Ok(())
    }
}

fn get_chat_admin_cache_key(chat: i64) -> String {
    format!("ca:{}", chat)
}

#[async_trait]
pub trait IsAdmin {
    async fn is_admin(&self, chat: &Chat) -> Result<bool>;
    async fn admin_or_die(&self, chat: &Chat) -> Result<()>;
}

#[async_trait]
pub trait IsGroupAdmin {
    async fn group_admin_or_die(&self) -> Result<()>;
    async fn is_group_admin(&self) -> Result<bool>;
}

#[async_trait]
pub trait GetCachedAdmins {
    async fn get_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn is_user_admin(&self, user: i64) -> Result<Option<ChatMember>>;
    async fn promote(&self, user: i64) -> Result<()>;
    async fn demote(&self, user: i64) -> Result<()>;
}

#[async_trait]
impl IsGroupAdmin for Message {
    async fn is_group_admin(&self) -> Result<bool> {
        if let Some(user) = self.get_from() {
            user.is_admin(self.get_chat_ref()).await
        } else {
            Ok(false)
        }
    }

    async fn group_admin_or_die(&self) -> Result<()> {
        is_group_or_die(self.get_chat_ref()).await?;
        self_admin_or_die(self.get_chat_ref()).await?;

        if self.is_group_admin().await? {
            Ok(())
        } else if let Some(user) = self.get_from() {
            let lang = get_chat_lang(self.get_chat().get_id()).await?;
            let msg = lang_fmt!(lang, "lackingadminrights", user.name_humanreadable());
            Err(BotError::speak(msg, self.get_chat().get_id()))
        } else {
            Err(BotError::Generic("not admin".to_owned()))
        }
    }
}

#[async_trait]
impl IsAdmin for User {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        Ok(chat.is_user_admin(self.get_id()).await?.is_some())
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if self.is_admin(chat).await? {
            Ok(())
        } else {
            let lang = get_chat_lang(chat.get_id()).await?;
            let msg = lang_fmt!(lang, "lackingadminrights", self.name_humanreadable());
            Err(BotError::speak(msg, chat.get_id()))
        }
    }
}

#[async_trait]
impl<'a> IsAdmin for Option<Cow<'a, User>> {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        if let Some(user) = self {
            Ok(chat.is_user_admin(user.get_id()).await?.is_some())
        } else {
            Ok(false)
        }
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if let Some(user) = self {
            if user.is_admin(chat).await? {
                Ok(())
            } else {
                let lang = get_chat_lang(chat.get_id()).await?;
                let msg = lang_fmt!(
                    lang,
                    "lackingadminrights",
                    user.get_username_ref()
                        .unwrap_or(user.get_id().to_string().as_str())
                );
                Err(BotError::speak(msg, chat.get_id()))
            }
        } else {
            Err(BotError::Generic("fail".to_owned()))
        }
    }
}

#[async_trait]
impl IsAdmin for i64 {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        Ok(chat.is_user_admin(*self).await?.is_some())
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if self.is_admin(chat).await? {
            Ok(())
        } else {
            let lang = get_chat_lang(chat.get_id()).await?;
            let msg = if let Some(user) = self.get_cached_user().await? {
                lang_fmt!(
                    lang,
                    "lackingadminrights",
                    user.get_username_ref().unwrap_or(self.to_string().as_str())
                )
            } else {
                lang_fmt!(lang, "lackingadminrights", self)
            };

            Err(BotError::speak(msg, chat.get_id()))
        }
    }
}

pub async fn update_self_admin(update: &UpdateExt) -> Result<()> {
    if let UpdateExt::MyChatMember(member) = update {
        let key = get_chat_admin_cache_key(member.get_chat().get_id());
        match member.get_new_chat_member_ref() {
            ChatMember::ChatMemberAdministrator(ref admin) => {
                log::info!("bot updated to admin");
                let user_id = admin.get_user().get_id();
                let admin = RedisStr::new(&admin)?;
                REDIS.sq(|q| q.hset(&key, user_id, admin)).await?;
            }
            ChatMember::ChatMemberOwner(ref owner) => {
                log::info!("Im soemhow the owner. What?");
                let user_id = owner.get_user().get_id();
                let admin = RedisStr::new(&owner)?;
                REDIS.sq(|q| q.hset(&key, user_id, admin)).await?;
            }
            mamber => {
                log::info!("Im not admin anymore ;(");
                let user_id = mamber.get_user().get_id();
                REDIS.sq(|q| q.hdel(&key, user_id)).await?;
            }
        }
    }

    Ok(())
}

#[async_trait]
impl GetCachedAdmins for Chat {
    async fn get_cached_admins(&self) -> Result<HashMap<i64, ChatMember>> {
        let key = get_chat_admin_cache_key(self.get_id());
        let admins: Option<HashMap<i64, RedisStr>> = REDIS.sq(|q| q.hgetall(&key)).await?;
        if let Some(admins) = admins {
            let admins = admins
                .into_iter()
                .map(|(k, v)| (k, v.get::<ChatMember>()))
                .try_fold(HashMap::new(), |mut acc, (k, v)| {
                    acc.insert(k, v?);
                    Ok::<_, BotError>(acc)
                })?;
            Ok(admins)
        } else {
            self.refresh_cached_admins().await
        }
    }

    async fn is_user_admin(&self, user: i64) -> Result<Option<ChatMember>> {
        let key = get_chat_admin_cache_key(self.get_id());
        let (exists, admin): (bool, Option<RedisStr>) = REDIS
            .pipe(|q| q.atomic().exists(&key).hget(&key, user))
            .await?;
        if exists {
            if let Some(user) = admin {
                Ok(Some(user.get::<ChatMember>()?))
            } else {
                Ok(None)
            }
        } else {
            Ok(self.refresh_cached_admins().await?.remove(&user))
        }
    }

    async fn promote(&self, user: i64) -> Result<()> {
        TG.client()
            .build_promote_chat_member(self.get_id(), user)
            .can_manage_chat(true)
            .can_restrict_members(true)
            .can_post_messages(true)
            .can_edit_messages(true)
            .can_manage_video_chats(true)
            .can_change_info(true)
            .can_invite_users(true)
            .can_pin_messages(true)
            .can_delete_messages(true)
            .can_promote_members(true)
            .build()
            .await?;

        let mamber = TG
            .client()
            .build_get_chat_member(self.get_id(), user)
            .build()
            .await?;

        let key = get_chat_admin_cache_key(self.get_id());
        let cm = RedisStr::new(&mamber)?;
        REDIS.sq(|q| q.hset(&key, user, cm)).await?;
        Ok(())
    }

    async fn demote(&self, user: i64) -> Result<()> {
        TG.client()
            .build_promote_chat_member(self.get_id(), user)
            .can_manage_chat(false)
            .can_restrict_members(false)
            .can_post_messages(false)
            .can_edit_messages(false)
            .can_manage_video_chats(false)
            .can_change_info(false)
            .can_invite_users(false)
            .can_pin_messages(false)
            .can_delete_messages(false)
            .can_promote_members(false)
            .build()
            .await?;
        let key = get_chat_admin_cache_key(self.get_id());
        REDIS.sq(|q| q.hdel(&key, user)).await?;
        Ok(())
    }

    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>> {
        if let Err(_) = is_group_or_die(self).await {
            return Ok(HashMap::new());
        }
        let admins = TG
            .client()
            .build_get_chat_administrators(self.get_id())
            .chat_id(self.get_id())
            .build()
            .await?;
        let res = admins
            .iter()
            .cloned()
            .map(|cm| (cm.get_user().get_id(), cm))
            .collect::<HashMap<i64, ChatMember>>();
        let mut admins = admins.into_iter().map(|cm| (cm.get_user().get_id(), cm));
        let lockkey = format!("aclock:{}", self.get_id());
        if !REDIS.sq(|q| q.exists(&lockkey)).await? {
            let key = get_chat_admin_cache_key(self.get_id());

            REDIS
                .try_pipe(|q| {
                    q.set(&lockkey, true);
                    q.expire(&lockkey, Duration::minutes(10).num_seconds() as usize);
                    admins.try_for_each(|(id, cm)| {
                        q.hset(&key, id, RedisStr::new(&cm)?);
                        Ok::<(), BotError>(())
                    })?;
                    Ok(q.expire(&key, Duration::hours(48).num_seconds() as usize))
                })
                .await?;
            Ok(res)
        } else {
            let lang = get_chat_lang(self.get_id()).await?;
            Err(BotError::speak(lang_fmt!(lang, "cachewait"), self.get_id()))
        }
    }
}
