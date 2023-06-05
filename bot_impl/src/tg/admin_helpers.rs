//! Helper functions and types for performing common admin actions
//! like banning, muting, warning etc.
//!
//! this module depends on the `static` module for access to the database, redis,
//! and telegram client.

use std::{borrow::Cow, collections::VecDeque};

use crate::{
    persist::{
        admin::{
            actions::{self, ActionType},
            approvals, warns,
        },
        core::{dialogs, users},
        redis::{default_cache_query, CachedQuery, CachedQueryTrait, RedisCache, RedisStr},
    },
    statics::{CONFIG, DB, ME, REDIS, TG},
    util::error::{BotError, Result},
    util::string::{get_chat_lang, Speak},
};

use botapi::gen_types::{
    Chat, ChatMember, ChatMemberUpdated, ChatPermissions, ChatPermissionsBuilder, Message,
    UpdateExt, User,
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
    permissions::{GetCachedAdmins, IsAdmin},
    user::{get_user_username, GetUser, Username},
};

/// Helper type for a named pair of chat and  user api types. Used to refer to a
/// chat member
pub struct ChatUser<'a> {
    pub chat: Cow<'a, Chat>,
    pub user: Cow<'a, User>,
}

/// Trait for getting a ChatUser from either a type containing both chat and user
/// or a chat (with provided extra user)
pub trait IntoChatUser {
    fn get_chatuser<'a>(&'a self) -> Option<ChatUser<'a>>;
    fn get_chatuser_user<'a>(&'a self, user: Cow<'a, User>) -> ChatUser<'a>;
}

/// Telegram's method for parsing a user's left or joined status from an Update
/// is very confusing. This enum simplifies this along with the UpdateHelpers trait
pub enum UserChanged<'a> {
    UserJoined(&'a ChatMemberUpdated),
    UserLeft(&'a ChatMemberUpdated),
}

impl<'a> UserChanged<'a> {
    /// Get a chat from a UserChanged enum since all varients contain a Chat
    pub fn get_chat(&'a self) -> &'a Chat {
        match self {
            UserChanged::UserJoined(m) => m.get_chat_ref(),
            UserChanged::UserLeft(m) => m.get_chat_ref(),
        }
    }
}

/// Trait for extending UpdateExt with helper functions to simplify parsing
pub trait UpdateHelpers {
    /// Since telegram requires a lot of different cases to determine whether an
    /// update is a 'chat left' or 'chat joined' event we simplify it by parsing to a
    /// UserChanged type
    fn user_event<'a>(&'a self) -> Option<UserChanged<'a>>;
}

impl UpdateHelpers for UpdateExt {
    /// Since telegram requires a lot of different cases to determine whether an
    /// update is a 'chat left' or 'chat joined' event we simplify it by parsing to a
    /// UserChanged type
    fn user_event<'a>(&'a self) -> Option<UserChanged<'a>> {
        if let UpdateExt::ChatMember(member) = self {
            if member.get_from().get_id() == ME.get().unwrap().get_id() {
                return None;
            }
            // log::info!(
            //     "welcome \nold: {:?}\nnew {:?}",
            //     member.get_old_chat_member_ref(),
            //     member.get_new_chat_member_ref()
            // );
            let old_left = match member.get_old_chat_member_ref() {
                ChatMember::ChatMemberLeft(_) => true,
                ChatMember::ChatMemberBanned(_) => true,
                ChatMember::ChatMemberRestricted(res) => !res.get_is_member(),
                _ => false,
            };

            let new_left = match member.get_new_chat_member_ref() {
                ChatMember::ChatMemberLeft(_) => true,
                ChatMember::ChatMemberBanned(_) => true,
                ChatMember::ChatMemberRestricted(res) => !res.get_is_member(),
                _ => false,
            };

            if old_left && !new_left {
                Some(UserChanged::UserJoined(member))
            } else {
                Some(UserChanged::UserLeft(member))
            }
        } else {
            None
        }
    }
}

/// Trait for telegram objects that can be deleted after a delay.
/// Meant to be used as an extension trait
pub trait DeleteAfterTime {
    /// Delete the object after the specified duration
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

/// Returns true if the bot is admin in a chat
pub async fn is_self_admin(chat: &Chat) -> Result<bool> {
    let me = ME.get().unwrap();
    Ok(chat.is_user_admin(me.get_id()).await?.is_some())
}

/// Returns true if a chat is a direct message with a user
pub fn is_dm(chat: &Chat) -> bool {
    chat.get_tg_type() == "private"
}

/// Gets the redis key string for caching admin actins
fn get_action_key(user: i64, chat: i64) -> String {
    format!("act:{}:{}", user, chat)
}

/// Gets the redis key string for caching warns
fn get_warns_key(user: i64, chat: i64) -> String {
    format!("warns:{}:{}", user, chat)
}

/// Kicks a user from the specified chat. This is implemented
// by banning then immmediately unbanning
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

/// Kicks the sender of a given message from the chat
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

/// Restrict a given user in a given chat for the provided duration.
/// If the user is not currently in the chat the permission change is
/// queued until the user joins
pub async fn change_permissions(
    chat: &Chat,
    user: i64,
    permissions: &ChatPermissions,
    time: Option<Duration>,
) -> Result<()> {
    let me = ME.get().unwrap();
    let lang = get_chat_lang(chat.get_id()).await?;
    if user.is_admin(chat).await? {
        Err(BotError::speak(lang_fmt!(lang, "muteadmin"), chat.get_id()))
    } else {
        if user == me.get_id() {
            chat.speak(lang_fmt!(lang, "mutemyself")).await?;
            Err(BotError::speak(
                lang_fmt!(lang, "mutemyself"),
                chat.get_id(),
            ))
        } else {
            if let Some(time) = time.map(|t| Utc::now().checked_add_signed(t)).flatten() {
                TG.client()
                    .build_restrict_chat_member(chat.get_id(), user, permissions)
                    .until_date(time.timestamp())
                    .build()
                    .await?;
            } else {
                TG.client()
                    .build_restrict_chat_member(chat.get_id(), user, permissions)
                    .build()
                    .await?;
            }
            let time = time.map(|t| Utc::now().checked_add_signed(t)).flatten();
            update_actions_permissions(user, chat, permissions, time).await?;
            Ok(())
        }
    }
}

/// Runs the provided function with parameters specifying a user and message parsed from the
/// arguments of a command. This is used to allows users to specify messages to interact with
/// using either mentioning a user via an @ handle or text mention or by replying to a message.
/// The user mentioned OR the sender of the message that is replied to is passed to the callback
/// function along with the remaining args and the message itself
pub async fn action_message<'a, F>(
    message: &'a Message,
    entities: &Entities<'a>,
    args: Option<&'a TextArgs<'a>>,
    action: F,
) -> Result<i64>
where
    for<'b> F: FnOnce(&'b Message, i64, Option<ArgSlice<'b>>) -> BoxFuture<'b, Result<()>>,
{
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    if let Some(user) = message
        .get_reply_to_message_ref()
        .map(|v| v.get_from())
        .flatten()
    {
        action(&message, user.get_id(), args.map(|a| a.as_slice())).await?;
        Ok(user.get_id())
    } else {
        match entities.front() {
            Some(EntityArg::Mention(name)) => {
                if let Some(user) = get_user_username(name).await? {
                    action(
                        message,
                        user.get_id(),
                        args.map(|a| a.pop_slice()).flatten(),
                    )
                    .await?;
                    Ok(user.get_id())
                } else {
                    return Err(BotError::speak(
                        lang_fmt!(lang, "usernotfound"),
                        message.get_chat().get_id(),
                    ));
                }
            }
            Some(EntityArg::TextMention(user)) => {
                action(
                    message,
                    user.get_id(),
                    args.map(|a| a.pop_slice()).flatten(),
                )
                .await?;
                Ok(user.get_id())
            }
            _ => {
                match args
                    .map(|v| {
                        v.args
                            .first()
                            .map(|v| i64::from_str_radix(v.get_text(), 10))
                    })
                    .flatten()
                {
                    Some(Ok(v)) => {
                        action(message, v, args.map(|a| a.pop_slice()).flatten()).await?;
                        Ok(v)
                    }
                    Some(Err(_)) => {
                        return Err(BotError::speak(
                            lang_fmt!(lang, "specifyuser"),
                            message.get_chat().get_id(),
                        ));
                    }
                    None => {
                        return Err(BotError::speak(
                            lang_fmt!(lang, "specifyuser"),
                            message.get_chat().get_id(),
                        ));
                    }
                }
            }
        }
    }
}

/// Parse a std::chrono::Duration from a human readable string (5m, 4d, etc)
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

/// Parse an std::chrono::Duration from a argument list
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

/// Persistantly change the permission of a user by using action_message syntax
pub async fn change_permissions_message<'a>(
    message: &Message,
    entities: &VecDeque<EntityArg<'a>>,
    permissions: ChatPermissions,
    args: &'a TextArgs<'a>,
) -> Result<i64> {
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

/// Issue a warning to a user, speaking in the chat as required. If the warn count
/// exceeds the currently configured count fetch the configured action and apply it
pub async fn warn_with_action(
    message: &Message,
    user: i64,
    reason: Option<&str>,
    duration: Option<Duration>,
) -> Result<(i32, i32)> {
    let dialog = dialog_or_default(message.get_chat_ref()).await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let time = dialog.warn_time.map(|t| Duration::seconds(t));
    let count = warn_user(message, user, reason.map(|v| v.to_owned()), &time).await?;
    let name = if let Some(user) = user.get_cached_user().await? {
        user.name_humanreadable()
    } else {
        user.to_string()
    };
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

/// Sets the duration after which warns expire for the provided chat
pub async fn set_warn_time(chat: &Chat, time: Option<i64>) -> Result<()> {
    let chat_id = chat.get_id();

    let model = dialogs::ActiveModel {
        chat_id: Set(chat_id),
        language: NotSet,
        chat_type: Set(chat.get_tg_type().into_owned()),
        warn_limit: NotSet,
        action_type: NotSet,
        warn_time: Set(time),
        can_send_messages: NotSet,
        can_send_audio: NotSet,
        can_send_video: NotSet,
        can_send_photo: NotSet,
        can_send_document: NotSet,
        can_send_video_note: NotSet,
        can_send_voice_note: NotSet,
        can_send_poll: NotSet,
        can_send_other: NotSet,
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

/// Sets the number of warns until an action is triggered for the provided chat
pub async fn set_warn_limit(chat: &Chat, limit: i32) -> Result<()> {
    let chat_id = chat.get_id();

    let model = dialogs::ActiveModel {
        chat_id: Set(chat_id),
        language: NotSet,
        chat_type: Set(chat.get_tg_type().into_owned()),
        warn_limit: Set(limit),
        action_type: NotSet,
        warn_time: NotSet,
        can_send_messages: NotSet,
        can_send_audio: NotSet,
        can_send_video: NotSet,
        can_send_photo: NotSet,
        can_send_document: NotSet,
        can_send_video_note: NotSet,
        can_send_voice_note: NotSet,
        can_send_poll: NotSet,
        can_send_other: NotSet,
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

/// Sets the action to be applied when the warn count is exceeeded, parsing
/// it from a string
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
        can_send_messages: NotSet,
        can_send_audio: NotSet,
        can_send_video: NotSet,
        can_send_photo: NotSet,
        can_send_document: NotSet,
        can_send_video_note: NotSet,
        can_send_voice_note: NotSet,
        can_send_poll: NotSet,
        can_send_other: NotSet,
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

/// Gets pending permissions to be applied to a user. This map onto telegram's built-in
/// restrictions with the addition of a 'ban' permission.
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

/// Helper function to handle a ban action after warn limit is exceeded.
/// Automatically sends localized string
pub async fn warn_ban(
    message: &Message,
    user: i64,
    count: i32,
    duration: Option<Duration>,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    ban(message, user, duration).await?;
    message
        .reply_fmt(entity_fmt!(
            lang,
            message.get_chat().get_id(),
            "warnban",
            MarkupType::Text.text(&count.to_string()),
            user.mention().await?,
        ))
        .await?;
    Ok(())
}

/// Helper function to handle a mute action after warn limit is exceeded.
/// Automatically sends localized string
pub async fn warn_mute(
    message: &Message,
    user: i64,
    count: i32,
    duration: Option<Duration>,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    mute(message.get_chat_ref(), user, duration).await?;

    let mention = user.mention().await?;
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

pub async fn warn_shame(message: &Message, _user: i64, _count: i32) -> Result<()> {
    message.speak("shaming not implemented").await?;

    Ok(())
}

/// Gets a list of all warns for the current user in the given chat (from message)
pub async fn get_warns(message: &Message, user_id: i64) -> Result<Vec<warns::Model>> {
    let chat_id = message.get_chat().get_id();
    let key = get_warns_key(user_id, message.get_chat().get_id());
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
            let (exists, count): (bool, Vec<RedisStr>) =
                REDIS.pipe(|q| q.exists(&key).smembers(&key)).await?;
            Ok((
                exists,
                count
                    .into_iter()
                    .filter_map(|v| v.get::<warns::Model>().ok())
                    .collect(),
            ))
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

/// Gets the number of warns a user has in the given chat (from message)
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
                let (exists, count): (bool, u64) =
                    REDIS.pipe(|q| q.exists(&key).llen(&key)).await?;
                Ok((exists, count))
            },
            |_, v| async move { Ok(v) },
        )
        .query(&key, &())
        .await?;
        Ok(r as i32)
    }
}

/// Removes all warns from a user in a chat
pub async fn clear_warns(chat: &Chat, user: i64) -> Result<()> {
    let key = get_warns_key(user, chat.get_id());
    REDIS.sq(|q| q.del(&key)).await?;
    warns::Entity::delete_many()
        .filter(
            warns::Column::ChatId
                .eq(chat.get_id())
                .and(warns::Column::UserId.eq(user)),
        )
        .exec(DB.deref().deref())
        .await?;
    Ok(())
}

/// Removes all restrictions on a user in a chat. This is persistent and
/// if the user is not present the changes will be applied on joining
pub async fn unmute(chat: &Chat, user: i64) -> Result<()> {
    let old = TG.client.get_chat(chat.get_id()).await?;
    let old = old.get_permissions().ok_or_else(|| {
        BotError::speak(
            "cannot unmute user, failed to get permissions",
            chat.get_id(),
        )
    })?;
    let mut new = ChatPermissionsBuilder::new();
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

    new = merge_permissions(&permissions, new);
    new = merge_permissions(&old, new);

    change_permissions(chat, user, &new.build(), None).await?;
    Ok(())
}

/// Restricts a user in a given chat. If the user not present the restriction will be
/// applied when they join. If a duration is specified the restrictions will be removed
/// after the duration
pub async fn mute(chat: &Chat, user: i64, duration: Option<Duration>) -> Result<()> {
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

#[inline(always)]
fn get_approval_key(chat: &Chat, user: i64) -> String {
    format!("ap:{}:{}", chat.get_id(), user)
}

/// Adds a user to an allowlist so that all future moderation actions are ignored
pub async fn approve(chat: &Chat, user: &User) -> Result<()> {
    let testmodel = users::Entity::insert(users::ActiveModel {
        user_id: Set(user.get_id()),
        username: Set(user.get_username().map(|v| v.into_owned())),
    })
    .on_conflict(
        OnConflict::column(users::Column::UserId)
            .update_columns([users::Column::Username])
            .to_owned(),
    )
    .exec_with_returning(DB.deref())
    .await?;

    approvals::Entity::insert(
        approvals::Model {
            chat: chat.get_id(),
            user: user.get_id(),
        }
        .join_single(get_approval_key(chat, user.get_id()), Some(testmodel))
        .await?
        .0,
    )
    .on_conflict(
        OnConflict::columns([approvals::Column::Chat, approvals::Column::User])
            .update_columns([approvals::Column::Chat, approvals::Column::User])
            .to_owned(),
    )
    .exec(DB.deref())
    .await?;

    Ok(())
}

/// Removes a user from the approval allowlist, all future moderation actions will be applied
pub async fn unapprove(chat: &Chat, user: i64) -> Result<()> {
    approvals::Entity::delete(approvals::ActiveModel {
        chat: Set(chat.get_id()),
        user: Set(user),
    })
    .exec(DB.deref())
    .await?;

    let key = get_approval_key(chat, user);

    REDIS.sq(|q| q.del(&key)).await?;
    Ok(())
}

/// Checks if a user should be ignored when applying moderation. All modules should honor
/// this when moderating
pub async fn is_approved(chat: &Chat, user: &User) -> Result<bool> {
    let chat_id = chat.get_id();
    let user_id = user.get_id();
    let key = get_approval_key(chat, user_id);
    let res = default_cache_query(
        |_, _| async move {
            let res = approvals::Entity::find_by_id((chat_id, user_id))
                .find_with_related(users::Entity)
                .all(DB.deref())
                .await?
                .pop();

            Ok(res.map(|(res, _)| res))
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?
    .is_some();

    Ok(res)
}

/// Gets a list of all approved users in the provided chat. Returns both user id and
/// human readable name
pub async fn get_approvals(chat: &Chat) -> Result<Vec<(i64, String)>> {
    let chat_id = chat.get_id();
    let res = approvals::Entity::find()
        .filter(approvals::Column::Chat.eq(chat_id))
        .find_with_related(users::Entity)
        .all(DB.deref())
        .await?;

    Ok(res
        .into_iter()
        .map(|(res, mut user)| {
            let id = res.user;
            let name = user
                .pop()
                .map(|v| v.username)
                .flatten()
                .unwrap_or_else(|| id.to_string());
            (id, name)
        })
        .collect())
}

fn merge_permissions(
    permissions: &ChatPermissions,
    mut new: ChatPermissionsBuilder,
) -> ChatPermissionsBuilder {
    if let Some(p) = permissions.get_can_send_messages() {
        new = new.set_can_send_messages(p);
    }

    if let Some(p) = permissions.get_can_send_audios() {
        new = new.set_can_send_audios(p);
    }

    if let Some(p) = permissions.get_can_send_documents() {
        new = new.set_can_send_documents(p);
    }

    if let Some(p) = permissions.get_can_send_photos() {
        new = new.set_can_send_photos(p);
    }

    if let Some(p) = permissions.get_can_send_videos() {
        new = new.set_can_send_videos(p);
    }

    if let Some(p) = permissions.get_can_send_video_notes() {
        new = new.set_can_send_video_notes(p);
    }

    if let Some(p) = permissions.get_can_send_polls() {
        new = new.set_can_send_polls(p);
    }

    if let Some(p) = permissions.get_can_send_voice_notes() {
        new = new.set_can_send_voice_notes(p);
    }

    if let Some(p) = permissions.get_can_send_other_messages() {
        new = new.set_can_send_other_messages(p);
    }

    new
}

/// Sets the default permissions for the current chat
pub async fn change_chat_permissions(chat: &Chat, permissions: &ChatPermissions) -> Result<()> {
    let current_perms = TG.client.get_chat(chat.get_id()).await?;
    let mut new = ChatPermissionsBuilder::new();
    let old = current_perms
        .get_permissions()
        .ok_or_else(|| BotError::speak("failed to get chat permissions", chat.get_id()))?;
    new = merge_permissions(&old, new);
    new = merge_permissions(permissions, new);
    let new = new.build();
    TG.client
        .build_set_chat_permissions(chat.get_id(), &new)
        .use_independent_chat_permissions(true)
        .build()
        .await?;
    Ok(())
}

/// Unbans a user, transparently handling anonymous channels
pub async fn unban(message: &Message, user: i64) -> Result<()> {
    if let Some(senderchat) = message.get_sender_chat() {
        TG.client()
            .build_unban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
            .build()
            .await?;
    } else {
        TG.client()
            .build_unban_chat_member(message.get_chat().get_id(), user)
            .build()
            .await?;
    }
    Ok(())
}

/// Bans the sender of a message, transparently handling anonymous channels.
/// if a duration is provided, the ban will be lifted after the duration
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

/// Bans a user in the given chat (from message), transparently handling anonymous channels.
/// if a duration is specified. the ban will be lifted
pub async fn ban(message: &Message, user: i64, duration: Option<Duration>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    if let Some(senderchat) = message.get_sender_chat() {
        TG.client()
            .build_ban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
            .build()
            .await?;
        let name = senderchat.name_humanreadable();
        if let Some(user) = user.get_cached_user().await? {
            let mention = MarkupType::TextMention(user).text(&name);
            message
                .speak_fmt(entity_fmt!(
                    lang,
                    message.get_chat().get_id(),
                    "banchat",
                    mention
                ))
                .await?;
        } else {
            message.speak(lang_fmt!(lang, "banchat", name)).await?;
        }
    }
    if user.is_admin(message.get_chat_ref()).await? {
        let banadmin = lang_fmt!(lang, "banadmin");
        return Err(BotError::speak(banadmin, message.get_chat().get_id()));
    } else {
        if let Some(duration) = duration.map(|v| Utc::now().checked_add_signed(v)).flatten() {
            TG.client()
                .build_ban_chat_member(message.get_chat().get_id(), user)
                .until_date(duration.timestamp())
                .build()
                .await?;
        } else {
            TG.client()
                .build_ban_chat_member(message.get_chat().get_id(), user)
                .build()
                .await?;
        }

        let mention = user.mention().await?;
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

/// Warns a user in the given chat, incrementing and returning the warn count.
/// if a reason is provided the reason is recorded with the warn. If a duration is provided
/// the warn will be lifted after the duration
pub async fn warn_user(
    message: &Message,
    user: i64,
    reason: Option<String>,
    duration: &Option<Duration>,
) -> Result<i32> {
    let chat_id = message.get_chat().get_id();
    let duration = duration.map(|v| Utc::now().checked_add_signed(v)).flatten();
    let model = warns::ActiveModel {
        id: NotSet,
        user_id: Set(user),
        chat_id: Set(chat_id),
        reason: Set(reason),
        expires: Set(duration),
    };
    let model = warns::Entity::insert(model)
        .exec_with_returning(DB.deref().deref())
        .await?;
    let model = RedisStr::new(&model)?;
    let key = get_warns_key(user, chat_id);
    let (_, _, count): ((), (), usize) = REDIS
        .pipe(|p| {
            p.sadd(&key, model)
                .expire(&key, CONFIG.timing.cache_timeout)
                .scard(&key)
        })
        .await?;

    Ok(count as i32)
}

/// Updates the current stored action with a user, either banning or unbanning.
/// the user is not immediately unbanned but the action is applied the next time the user is
/// seen
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

/// Sets the 'pending' flag on a stored action. Pending actions are applied the next time a user is seen
/// actions without pending set are ignored
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

/// Updates the current action for a user with new permissions.
/// these permissions will be applied the next time the user is seen
pub async fn update_actions_permissions(
    user: i64,
    chat: &Chat,
    permissions: &ChatPermissions,
    expires: Option<DateTime<Utc>>,
) -> Result<()> {
    let key = get_action_key(user, chat.get_id());

    let active = actions::ActiveModel {
        user_id: Set(user),
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

/// Checks an update for user interactions and applies the current action for the user
/// if it is pending. clearing the pending flag in the process
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

/// Checks if the provided user has a pending action, and applies it if needed.
/// afterwards, the pending flag is cleared
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

                unmute(&chat, user.get_id()).await?;
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

/// Updates the current actions with a raw ORM model
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

/// If the current chat is a group or supergroup (i.e. not a dm)
/// Warn the user and return Err
pub async fn is_dm_or_die(chat: &Chat) -> Result<()> {
    let lang = get_chat_lang(chat.get_id()).await?;
    if !is_dm(chat) {
        Err(BotError::speak(lang_fmt!(lang, "notdm"), chat.get_id()))
    } else {
        Ok(())
    }
}

/// Check if the group is a supergroup, and warn the user while returning error if it is not
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
