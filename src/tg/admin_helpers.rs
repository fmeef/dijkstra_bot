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
    util::error::{BotError, Fail, Result},
    util::string::{get_chat_lang, Speak},
};

use async_trait::async_trait;
use botapi::gen_types::{
    Chat, ChatMember, ChatMemberUpdated, ChatPermissions, ChatPermissionsBuilder, Document,
    InlineKeyboardButtonBuilder, MaybeInaccessibleMessage, Message, UpdateExt, User,
};
use bytes::Bytes;
use chrono::{DateTime, Duration, Utc};
use futures::Future;

use lazy_static::{__Deref, lazy_static};
use macros::{entity_fmt, lang_fmt};
use redis::AsyncCommands;
use reqwest::Response;
use sea_orm::{
    sea_query::OnConflict, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait,
    IntoActiveModel, ModelTrait, PaginatorTrait, QueryFilter,
};

use uuid::Uuid;

use super::{
    button::OnPush,
    command::{ArgSlice, Context, Entities, EntityArg, PopSlice},
    dialog::{dialog_or_default, get_dialog_key},
    markdown::MarkupType,
    permissions::{GetCachedAdmins, IsAdmin},
    user::{get_user_username, GetUser, Username},
};

lazy_static! {
    static ref VECDEQUE: Entities<'static> = VecDeque::new();
}

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
            } else if !old_left && new_left {
                Some(UserChanged::UserLeft(member))
            } else {
                None
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
        federation: NotSet,
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::WarnTime)
                .to_owned(),
        )
        .exec_with_returning(DB.deref())
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
        federation: NotSet,
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::WarnLimit)
                .to_owned(),
        )
        .exec_with_returning(DB.deref())
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
        _ => chat.fail(format!("Invalid mode {}", mode)),
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
        federation: NotSet,
    };

    let key = get_dialog_key(chat_id);
    let model = dialogs::Entity::insert(model)
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::ActionType)
                .to_owned(),
        )
        .exec_with_returning(DB.deref())
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

pub async fn warn_shame(message: &Message, _user: i64, _count: i32) -> Result<()> {
    message.speak("shaming not implemented").await?;

    Ok(())
}

/// Gets a list of all warns for the current user in the given chat (from message)
pub async fn get_warns(chat: &Chat, user_id: i64) -> Result<Vec<warns::Model>> {
    let chat_id = chat.get_id();
    let key = get_warns_key(user_id, chat_id);
    let r = CachedQuery::new(
        |_, _| async move {
            let count = warns::Entity::find()
                .filter(
                    warns::Column::UserId
                        .eq(user_id)
                        .and(warns::Column::ChatId.eq(chat_id)),
                )
                .all(DB.deref())
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
pub async fn get_warns_count(message: &Message, user: i64) -> Result<i32> {
    let chat_id = message.get_chat().get_id();
    let key = get_warns_key(user, message.get_chat().get_id());
    let v: Option<i32> = REDIS.sq(|q| q.scard(&key)).await?;
    if let Some(v) = v {
        Ok(v)
    } else {
        let r = CachedQuery::new(
            |_, _| async move {
                let count = warns::Entity::find()
                    .filter(
                        warns::Column::UserId
                            .eq(user)
                            .and(warns::Column::ChatId.eq(chat_id)),
                    )
                    .count(DB.deref())
                    .await?;
                Ok(count)
            },
            |key, _| async move {
                let (exists, count): (bool, u64) =
                    REDIS.pipe(|q| q.exists(&key).scard(&key)).await?;
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
        .exec(DB.deref())
        .await?;
    Ok(())
}

#[inline(always)]
fn get_approval_key(chat: &Chat, user: i64) -> String {
    format!("ap:{}:{}", chat.get_id(), user)
}

pub async fn insert_user(user: &User) -> Result<users::Model> {
    let testmodel = users::Entity::insert(users::ActiveModel {
        user_id: Set(user.get_id()),
        username: Set(user.get_username().map(|v| v.into_owned())),
        first_name: Set(user.get_first_name().into_owned()),
        last_name: Set(user.get_last_name().map(|v| v.into_owned())),
        is_bot: Set(user.get_is_bot()),
    })
    .on_conflict(
        OnConflict::column(users::Column::UserId)
            .update_columns([
                users::Column::Username,
                users::Column::FirstName,
                users::Column::LastName,
            ])
            .to_owned(),
    )
    .exec_with_returning(DB.deref())
    .await?;

    Ok(testmodel)
}

/// Adds a user to an allowlist so that all future moderation actions are ignored
pub async fn approve(chat: &Chat, user: &User) -> Result<()> {
    let testmodel = insert_user(user).await?;
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
        .ok_or_else(|| chat.fail_err("failed to get chat permissions"))?;
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

/// If the current chat is a group or supergroup (i.e. not a dm)
/// Warn the user and return Err
pub async fn is_dm_or_die(chat: &Chat) -> Result<()> {
    let lang = get_chat_lang(chat.get_id()).await?;
    if !is_dm(chat) {
        chat.fail(lang_fmt!(lang, "notdm"))
    } else {
        Ok(())
    }
}

/// Check if the group is a supergroup, and warn the user while returning error if it is not
pub async fn is_group_or_die(chat: &Chat) -> Result<()> {
    let lang = get_chat_lang(chat.get_id()).await?;
    match chat.get_tg_type().as_ref() {
        "private" => chat.fail(lang_fmt!(lang, "baddm")),
        "group" => chat.fail(lang_fmt!(lang, "notsupergroup")),
        _ => Ok(()),
    }
}

pub enum ActionMessage<'a> {
    Me(&'a Message),
    Reply(&'a Message),
}

impl<'a> ActionMessage<'a> {
    pub fn message(&'a self) -> &'a Message {
        match self {
            Self::Me(m) => m,
            Self::Reply(m) => m,
        }
    }
}

impl Context {
    /// Checks an update for user interactions and applies the current action for the user
    /// if it is pending. clearing the pending flag in the process
    pub async fn handle_pending_action_update<'a>(&self) -> Result<()> {
        match self.update() {
            UpdateExt::Message(ref message) => {
                if !is_dm(&message.get_chat()) {
                    if let Some(user) = message.get_from_ref() {
                        self.handle_pending_action(user).await?;
                    }
                }
            }
            _ => (),
        };

        Ok(())
    }

    /// Parse an std::chrono::Duration from a argument list
    pub fn parse_duration<'a>(&self, args: &Option<ArgSlice<'a>>) -> Result<Option<Duration>> {
        if let Some(args) = args {
            if let Some(thing) = args.args.first() {
                let head = &thing.get_text()[0..thing.get_text().len() - 1];
                let tail = &thing.get_text()[thing.get_text().len() - 1..];
                log::info!("head {} tail {}", head, tail);
                let head = match str::parse::<i64>(head) {
                    Err(_) => return self.fail("Enter a number"),
                    Ok(res) => res,
                };
                let res = match tail {
                    "m" => Duration::minutes(head),
                    "h" => Duration::hours(head),
                    "d" => Duration::days(head),
                    _ => return self.fail("Invalid time spec"),
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

    /// If the current chat is a group or supergroup (i.e. not a dm)
    /// Warn the user and return Err
    pub async fn is_dm_or_die(&self) -> Result<()> {
        if let Some(v) = self.get() {
            if !is_dm(v.chat) {
                self.fail(lang_fmt!(v.lang, "notdm"))
            } else {
                Ok(())
            }
        } else {
            Err(BotError::Generic("not a chat".to_owned()))
        }
    }

    /// Check if the group is a supergroup, and warn the user while returning error if it is not
    pub async fn is_group_or_die(&self) -> Result<()> {
        if let Some(v) = self.get() {
            let chat = v.chat;
            is_group_or_die(chat).await
        } else {
            Err(BotError::Generic("not a chat".to_owned()))
        }
    }

    /// Unbans a user, transparently handling anonymous channels
    pub async fn unban(&self, user: i64) -> Result<()> {
        if let Some(senderchat) = self.message()?.get_sender_chat() {
            TG.client()
                .build_unban_chat_sender_chat(self.try_get()?.chat.get_id(), senderchat.get_id())
                .build()
                .await?;
        } else {
            TG.client()
                .build_unban_chat_member(self.try_get()?.chat.get_id(), user)
                .build()
                .await?;
        }
        Ok(())
    }

    /// Helper function to handle a mute action after warn limit is exceeded.
    /// Automatically sends localized string
    pub async fn warn_mute(&self, user: i64, count: i32, duration: Option<Duration>) -> Result<()> {
        log::info!("warn_mute");
        let message = self.message()?;
        self.mute(user, self.try_get()?.chat, duration).await?;

        let mention = user.mention().await?;
        message
            .reply_fmt(entity_fmt!(self, "warnmute", count.to_string(), mention))
            .await?;

        Ok(())
    }

    /// Checks if the provided user has a pending action, and applies it if needed.
    /// afterwards, the pending flag is cleared
    pub async fn handle_pending_action(&self, user: &User) -> Result<()> {
        let chat = self.try_get()?.chat;
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

                    self.unmute(user.get_id(), chat).await?;
                    action.delete(DB.deref()).await?;
                    return Ok(());
                }
            }
            if action.pending {
                let name = user.name_humanreadable();
                if action.is_banned {
                    TG.client()
                        .build_ban_chat_member(chat.get_id(), user.get_id())
                        .build()
                        .await?;

                    let mention = MarkupType::TextMention(user.to_owned()).text(&name);
                    chat.speak_fmt(entity_fmt!(self, "banned", mention)).await?;
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

    /// Removes all restrictions on a user in a chat. This is persistent and
    /// if the user is not present the changes will be applied on joining
    pub async fn unmute(&self, user: i64, chat: &Chat) -> Result<()> {
        log::info!(
            "unmute for user {} in chat {}",
            user.cached_name().await?,
            chat.name_humanreadable()
        );
        let old = TG.client.get_chat(chat.get_id()).await?;
        let old = old.get_permissions().unwrap_or_else(|| {
            Cow::Owned(
                ChatPermissionsBuilder::new()
                    .set_can_send_messages(false)
                    .set_can_send_audios(false)
                    .set_can_send_documents(false)
                    .set_can_send_photos(false)
                    .set_can_send_videos(false)
                    .set_can_send_video_notes(false)
                    .set_can_send_polls(false)
                    .set_can_send_voice_notes(false)
                    .set_can_send_other_messages(false)
                    .build(),
            )
        });
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

        self.change_permissions_chat(user, chat, &new.build(), None)
            .await?;
        Ok(())
    }

    /// Restricts a user in a given chat. If the user not present the restriction will be
    /// applied when they join. If a duration is specified the restrictions will be removed
    /// after the duration
    pub async fn mute(&self, user: i64, chat: &Chat, duration: Option<Duration>) -> Result<()> {
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

        self.change_permissions_chat(user, chat, &permissions, duration)
            .await?;
        Ok(())
    }

    pub async fn change_permissions(
        &self,
        user: i64,
        permissions: &ChatPermissions,
        time: Option<Duration>,
    ) -> Result<()> {
        self.change_permissions_chat(user, self.try_get()?.chat, permissions, time)
            .await
    }

    /// Restrict a given user in a given chat for the provided duration.
    /// If the user is not currently in the chat the permission change is
    /// queued until the user joins
    pub async fn change_permissions_chat(
        &self,
        user: i64,
        chat: &Chat,
        permissions: &ChatPermissions,
        time: Option<Duration>,
    ) -> Result<()> {
        let me = ME.get().unwrap();
        if user == me.get_id() {
            self.fail(lang_fmt!(self.try_get()?.lang, "mutemyself"))
        } else if user.is_admin(chat).await? {
            self.fail(lang_fmt!(self.try_get()?.lang, "muteadmin"))
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

    /// Persistantly change the permission of a user by using action_message syntax
    pub async fn change_permissions_message(
        &self,
        permissions: ChatPermissions,
    ) -> Result<Option<i64>> {
        let me = self.clone();
        self.action_user(|ctx, user, args| async move {
            let duration = ctx.parse_duration(&args)?;
            me.change_permissions(user, &permissions, duration).await?;

            Ok(())
        })
        .await
    }

    pub async fn action_user<'a, F, Fut>(&'a self, action: F) -> Result<Option<i64>>
    where
        F: FnOnce(&'a Context, i64, Option<ArgSlice<'a>>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        self.action_message_some(|ctx, user, args, _| async move {
            if let Some(user) = user {
                action(ctx, user, args).await?;
            } else {
                return self.fail(lang_fmt!(ctx, "specifyuser"));
            }
            Ok(())
        })
        .await
    }

    pub async fn action_user_maybe<'a, F, Fut>(&'a self, action: F) -> Result<Option<i64>>
    where
        F: FnOnce(&'a Context, Option<i64>, Option<ArgSlice<'a>>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        self.action_message_some(|ctx, user, args, _| async move {
            action(ctx, user, args).await?;
            Ok(())
        })
        .await
    }

    pub async fn action_message<'a, F, Fut>(&'a self, action: F) -> Result<()>
    where
        F: FnOnce(&'a Context, ActionMessage<'a>, Option<ArgSlice<'a>>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let message = self.message()?;
        let args = self.try_get()?.command.as_ref().map(|a| &a.args);
        log::info!("action_message {:?}", args);

        if let Some(message) = message.get_reply_to_message_ref() {
            action(
                self,
                ActionMessage::Reply(message),
                args.map(|a| a.as_slice()),
            )
            .await?;
        } else {
            action(
                self,
                ActionMessage::Me(self.message()?),
                args.map(|a| a.as_slice()),
            )
            .await?;
        };
        Ok(())
    }

    /// Runs the provided function with parameters specifying a user and message parsed from the
    /// arguments of a command. This is used to allows users to specify messages to interact with
    /// using either mentioning a user via an @ handle or text mention or by replying to a message.
    /// The user mentioned OR the sender of the message that is replied to is passed to the callback
    /// function along with the remaining args and the message itself
    pub async fn action_message_some<'a, F, Fut>(&'a self, action: F) -> Result<Option<i64>>
    where
        F: FnOnce(&'a Context, Option<i64>, Option<ArgSlice<'a>>, ActionMessage<'a>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let message = self.message()?;
        let args = self.try_get()?.command.as_ref().map(|a| &a.args);
        log::info!("action_message {:?}", args);
        let entities = self
            .try_get()?
            .command
            .as_ref()
            .map(|e| &e.entities)
            .unwrap_or_else(|| &VECDEQUE);

        if let (Some(user), Some(message)) = (
            message
                .get_reply_to_message_ref()
                .map(|v| v.get_from())
                .flatten(),
            message.get_reply_to_message_ref(),
        ) {
            action(
                self,
                Some(user.get_id()),
                args.map(|a| a.as_slice()),
                ActionMessage::Reply(message),
            )
            .await?;
            Ok(Some(user.get_id()))
        } else {
            match entities.front() {
                Some(EntityArg::Mention(name)) => {
                    let user = get_user_username(name)
                        .await?
                        .ok_or_else(|| BotError::UserNotFound)?;
                    action(
                        self,
                        Some(user.get_id()),
                        args.map(|a| a.pop_slice_tail()).flatten(),
                        ActionMessage::Me(self.message()?),
                    )
                    .await?;
                    Ok(Some(user.get_id()))
                }
                Some(EntityArg::TextMention(user)) => {
                    action(
                        self,
                        Some(user.get_id()),
                        args.map(|a| a.pop_slice_tail()).flatten(),
                        ActionMessage::Me(self.message()?),
                    )
                    .await?;
                    Ok(Some(user.get_id()))
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
                            action(
                                self,
                                Some(v),
                                args.map(|a| a.pop_slice_tail()).flatten(),
                                ActionMessage::Me(self.message()?),
                            )
                            .await?;
                            Ok(Some(v))
                        }
                        Some(Err(_)) => {
                            action(
                                self,
                                None,
                                args.map(|a| a.pop_slice_tail()).flatten(),
                                ActionMessage::Me(self.message()?),
                            )
                            .await?;
                            Ok(None)
                        }
                        None => {
                            action(
                                self,
                                None,
                                args.map(|a| a.pop_slice_tail()).flatten(),
                                ActionMessage::Me(self.message()?),
                            )
                            .await?;
                            Ok(None)
                        }
                    }
                }
            }
        }
    }

    /// Issue a warning to a user, speaking in the chat as required. If the warn count
    /// exceeds the currently configured count fetch the configured action and apply it
    pub async fn warn_with_action(
        &self,
        user: i64,
        reason: Option<&str>,
        duration: Option<Duration>,
    ) -> Result<(i32, i32)> {
        let message = self.message()?;
        let dialog = dialog_or_default(message.get_chat_ref()).await?;
        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        let time = dialog.warn_time.map(|t| Duration::seconds(t));
        let (count, model) = warn_user(
            message,
            user,
            reason.map(|v| v.to_owned()),
            &time,
            dialog.warn_limit,
        )
        .await?;

        if count >= dialog.warn_limit {
            match dialog.action_type {
                actions::ActionType::Mute => self.warn_mute(user, count, duration).await,
                actions::ActionType::Ban => self.warn_ban(user, count, duration).await,
                actions::ActionType::Shame => warn_shame(message, user, count).await,
                actions::ActionType::Warn => Ok(()),
                actions::ActionType::Delete => Ok(()),
            }?;
        } else if let Some(model) = model {
            let name = user.mention().await?;
            let mut text = if let Some(_) = reason {
                entity_fmt!(
                    self,
                    "warnreason",
                    name,
                    count.to_string(),
                    dialog.warn_limit.to_string()
                )
            } else {
                entity_fmt!(
                    self,
                    "warn",
                    name,
                    count.to_string(),
                    dialog.warn_limit.to_string()
                )
            };
            text.builder.filling = true;
            text.builder = text.builder.chatuser(Some(
                &message.get_chatuser_user(Cow::Owned(
                    user.get_cached_user()
                        .await?
                        .ok_or_else(|| self.fail_err(lang_fmt!(lang, "failwarn")))?,
                )),
            ));
            let button_text = lang_fmt!(lang, "removewarn");

            let button = InlineKeyboardButtonBuilder::new(button_text)
                .set_callback_data(Uuid::new_v4().to_string())
                .build();
            let model = model.id;
            button.on_push_multi(move |cb| async move {
                if let Some(MaybeInaccessibleMessage::Message(message)) = cb.get_message_ref() {
                    let chat = message.get_chat_ref();
                    if cb.get_from().is_admin(chat).await? {
                        let key = get_warns_key(user, chat.get_id());
                        if let Some(res) = warns::Entity::find_by_id(model).one(DB.deref()).await? {
                            let st = RedisStr::new(&res)?;
                            res.delete(DB.deref()).await?;
                            REDIS.sq(|q| q.srem(&key, st)).await?;
                        }
                        TG.client
                            .build_edit_message_reply_markup()
                            .message_id(message.get_message_id())
                            .chat_id(chat.get_id())
                            .build()
                            .await?;
                        TG.client
                            .build_edit_message_text("Warn removed")
                            .message_id(message.get_message_id())
                            .chat_id(chat.get_id())
                            .build()
                            .await?;
                        TG.client
                            .build_answer_callback_query(cb.get_id_ref())
                            .build()
                            .await?;

                        Ok(true)
                    } else {
                        TG.client
                            .build_answer_callback_query(cb.get_id_ref())
                            .show_alert(true)
                            .text("User is not admin")
                            .build()
                            .await?;
                        Ok(false)
                    }
                } else {
                    Ok(true)
                }
            });

            if let Some(reason) = reason {
                text.builder.text(&reason);
            }
            text.builder.buttons.button(button);
            message.reply_fmt(text).await?;
        }
        Ok((count, dialog.warn_limit))
    }

    /// Helper function to handle a ban action after warn limit is exceeded.
    /// Automatically sends localized string
    pub async fn warn_ban(&self, user: i64, count: i32, duration: Option<Duration>) -> Result<()> {
        log::info!("warn_ban");
        let message = self.message()?;
        self.ban(user, duration).await?;
        message
            .reply_fmt(entity_fmt!(
                self,
                "warnban",
                count.to_string(),
                user.mention().await?,
            ))
            .await?;
        Ok(())
    }

    /// Bans a user in the given chat (from message), transparently handling anonymous channels.
    /// if a duration is specified. the ban will be lifted
    pub async fn ban(&self, user: i64, duration: Option<Duration>) -> Result<()> {
        let message = self.message()?;
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
                    .speak_fmt(entity_fmt!(self, "banchat", mention))
                    .await?;
            } else {
                message.speak(lang_fmt!(lang, "banchat", name)).await?;
            }
        }

        let me = ME.get().unwrap();

        if user == me.get_id() {
            return self.fail(lang_fmt!(lang, "banmyself"));
        } else if user.is_admin(message.get_chat_ref()).await? {
            let banadmin = lang_fmt!(lang, "banadmin");
            return self.fail(banadmin);
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
                .speak_fmt(entity_fmt!(self, "banned", mention))
                .await?;
        }
        Ok(())
    }
}

/// Warns a user in the given chat, incrementing and returning the warn count.
/// if a reason is provided the reason is recorded with the warn. If a duration is provided
/// the warn will be lifted after the duration
pub async fn warn_user(
    message: &Message,
    user: i64,
    reason: Option<String>,
    duration: &Option<Duration>,
    limit: i32,
) -> Result<(i32, Option<warns::Model>)> {
    let chat_id = message.get_chat().get_id();
    let duration = duration.map(|v| Utc::now().checked_add_signed(v)).flatten();
    let model = warns::ActiveModel {
        id: NotSet,
        user_id: Set(user),
        chat_id: Set(chat_id),
        reason: Set(reason),
        expires: Set(duration),
    };
    let count = get_warns_count(message, user).await?;
    if count >= limit {
        return Ok((count as i32, None));
    }
    let model = warns::Entity::insert(model)
        .exec_with_returning(DB.deref())
        .await?;
    let m = RedisStr::new(&model)?;
    let key = get_warns_key(user, chat_id);
    let (_, _, count): ((), (), usize) = REDIS
        .pipe(|p| {
            p.sadd(&key, m)
                .expire(&key, CONFIG.timing.cache_timeout)
                .scard(&key)
        })
        .await?;

    Ok((count as i32, Some(model)))
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
                .update_columns([
                    actions::Column::IsBanned,
                    actions::Column::Expires,
                    actions::Column::Pending,
                ])
                .to_owned(),
        )
        .exec_with_returning(DB.deref())
        .await?;

    res.cache(key).await?;
    Ok(())
}

/// Helper trait to convert emptystrings to Options
pub trait StrOption
where
    Self: Sized,
{
    fn none_if_empty(self) -> Option<Self>;
}

impl<T> StrOption for T
where
    T: Sized + AsRef<str>,
{
    fn none_if_empty(self) -> Option<Self> {
        if self.as_ref().len() == 0 {
            None
        } else {
            Some(self)
        }
    }
}

#[async_trait]
pub trait FileGetter {
    async fn get_bytes(&self) -> Result<Bytes>;
    async fn get_text(&self) -> Result<String>;
}

#[async_trait]
impl FileGetter for Document {
    async fn get_bytes(&self) -> Result<Bytes> {
        let file = TG
            .client
            .build_get_file(self.get_file_id_ref())
            .build()
            .await?;
        let path = file
            .get_file_path()
            .ok_or_else(|| BotError::Generic("Document file path missing".to_owned()))?;

        Ok(get_file(&path).await?)
    }

    async fn get_text(&self) -> Result<String> {
        let file = TG
            .client
            .build_get_file(self.get_file_id_ref())
            .build()
            .await?;
        let path = file
            .get_file_path()
            .ok_or_else(|| BotError::Generic("Docuemnt file path missing".to_owned()))?;
        Ok(get_file_text(&path).await?)
    }
}

async fn get_file_body(path: &str) -> Result<Response> {
    let path = format!("https://api.telegram.org/file/bot{}/{}", TG.token, path);
    let body = reqwest::get(path).await.map_err(|err| err.without_url())?;
    Ok(body)
}

/// Get a file from the boi api
/// <https://api.telegram.org/file/bot/path>
pub async fn get_file(path: &str) -> Result<Bytes> {
    let body = get_file_body(path).await?;
    let body = body.bytes().await?;
    Ok(body)
}

/// Get a file from the bot api as text
/// <https://api.telegram.org/file/bot/path>
pub async fn get_file_text(path: &str) -> Result<String> {
    let body = get_file_body(path).await?;
    let text = body.text().await?;
    Ok(text)
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
        .exec_with_returning(DB.deref())
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

    log::info!("update_actions_permissions {:?}", active);

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
        .exec_with_returning(DB.deref())
        .await?;

    res.cache(key).await?;

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
        .exec(DB.deref())
        .await?;
    Ok(())
}
