//! Various functions for caching user details and group membership.
//! This is intended to reduce the number of telegram requests, user information
//! stored in persistent database, and to allow reverse lookup of @ handles

use std::borrow::Cow;

use crate::persist::redis::RedisStr;
use crate::statics::{CONFIG, REDIS, TG};
use crate::util::error::Result;
use async_trait::async_trait;
use botapi::gen_types::{Chat, MessageOrigin, UpdateExt, User};
use redis::AsyncCommands;

use super::markdown::{Markup, MarkupType};

fn get_user_cache_key(user: i64) -> String {
    format!("usrc:{}", user)
}

fn get_username_cache_key(username: &str) -> String {
    format!("uname:{}", username)
}

fn get_chat_cache_key(chat: i64) -> String {
    format!("chat:{}", chat)
}

/// Get the user for this bot. This function just caches the getMe telegram API call
pub async fn get_me() -> Result<User> {
    let me_key = "user_me";
    REDIS
        .query(|mut q| async move {
            let me: Option<RedisStr> = q.get(&me_key).await?;
            if let Some(me) = me {
                Ok(me.get()?)
            } else {
                let me = TG.client().get_me().await?;
                let me_str = RedisStr::new(&me)?;
                q.set(&me_key, me_str).await?;
                Ok(me)
            }
        })
        .await
}

/// Record a user in redis for later lookup
pub(crate) async fn record_cache_user(user: &User) -> Result<()> {
    let key = get_user_cache_key(user.get_id());
    let st = RedisStr::new(user)?;
    if let Some(username) = user.get_username() {
        let uname = get_username_cache_key(&username);
        REDIS
            .pipe(|p| {
                p.set(&key, st)
                    .expire(&key, CONFIG.timing.cache_timeout)
                    .set(&uname, user.get_id())
                    .expire(&uname, CONFIG.timing.cache_timeout)
            })
            .await?;
    } else {
        REDIS
            .pipe(|p| p.set(&key, st).expire(&key, CONFIG.timing.cache_timeout))
            .await?;
    }
    Ok(())
}

/// Record a chat in redis for later lookup
pub async fn record_cache_chat(chat: &Chat) -> Result<()> {
    let key = get_chat_cache_key(chat.get_id());
    let st = RedisStr::new(chat)?;
    REDIS
        .pipe(|p| p.set(&key, st).expire(&key, CONFIG.timing.cache_timeout))
        .await?;
    Ok(())
}

/// Parse an update for users and chats and record them as needed
pub async fn record_cache_update(update: &UpdateExt) -> Result<()> {
    if let Some(user) = RecordUser::get_user(update) {
        record_cache_user(&user).await?;
    }
    if let UpdateExt::Message(m) = update {
        if let Some(m) = m.get_reply_to_message() {
            if let Some(user) = m.get_from() {
                user.record_user().await?;
            }
            if let Some(MessageOrigin::MessageOriginUser(m)) = m.get_forward_origin_ref() {
                m.get_sender_user().record_user().await?;
            }
        }

        if let Some(MessageOrigin::MessageOriginUser(m)) = m.get_forward_origin_ref() {
            m.get_sender_user().record_user().await?;
        }
    }
    Ok(())
}

/// get a cached user by id
pub async fn get_user(user: i64) -> Result<Option<User>> {
    let key = get_user_cache_key(user);
    let model: Option<RedisStr> = REDIS.sq(|p| p.get(&key)).await?;
    if let Some(model) = model {
        Ok(Some(model.get()?))
    } else {
        Ok(None)
    }
}

/// get a cached user by username
pub async fn get_user_username<T: AsRef<str>>(username: T) -> Result<Option<User>> {
    let username = username.as_ref();
    let key = get_username_cache_key(username);
    let id: Option<RedisStr> = REDIS
        .query(|mut q| async move {
            let id: Option<i64> = q.get(&key).await?;
            let res = if let Some(id) = id {
                let key = get_user_cache_key(id);
                q.get(&key).await?
            } else {
                None
            };
            Ok(res)
        })
        .await?;

    if let Some(id) = id {
        Ok(Some(id.get::<User>()?))
    } else {
        Ok(None)
    }
}

/// get a cached chat by chatId
pub async fn get_chat(chat: i64) -> Result<Option<Chat>> {
    let key = get_chat_cache_key(chat);
    let model: Option<RedisStr> = REDIS.sq(|p| p.get(&key)).await?;
    if let Some(model) = model {
        Ok(Some(model.get()?))
    } else {
        Ok(None)
    }
}

/// extension trait for getting human readable names from telegram objects
pub trait Username {
    /// get the human readable name, often either the display name, @ handle, or id number
    fn name_humanreadable<'a>(&'a self) -> String;
}

/// extension trait for recording a chat to redis
#[async_trait]
pub trait RecordChat {
    /// record the chat to redis
    async fn record_chat(&self) -> Result<()>;
}

/// extension trait for getting full cached information for a chat
#[async_trait]
pub trait GetChat {
    /// get the chat information
    async fn get_chat(&self) -> Result<Option<Chat>>;
}

/// extension trait for recording and retrieving a user from redis cache
#[async_trait]
pub trait RecordUser {
    /// helper to get the user from the value (not from cache)
    fn get_user<'a>(&'a self) -> Option<Cow<'a, User>>;

    /// record this user to redis. Does nothing if full information is not present
    async fn record_user(&self) -> Result<()>;
}

/// extension trait for getting the full cached information of a user
#[async_trait]
pub trait GetUser {
    /// Get the user's full information from redis cache
    async fn get_cached_user(&self) -> Result<Option<User>>;

    async fn cached_name(&self) -> Result<String>;

    async fn mention(&self) -> Result<Markup<String>>;
}

impl Username for User {
    fn name_humanreadable<'a>(&'a self) -> String {
        self.get_username()
            .map(|v| format!("@{}", v))
            .unwrap_or_else(|| self.get_id().to_string())
    }
}

impl Username for Chat {
    fn name_humanreadable<'a>(&'a self) -> String {
        self.get_title().map(|v| v.into_owned()).unwrap_or_else(|| {
            self.get_username()
                .map(|v| v.into_owned())
                .unwrap_or_else(|| self.get_id().to_string())
        })
    }
}

impl From<&User> for crate::persist::core::users::Model {
    fn from(user: &User) -> Self {
        Self {
            user_id: user.get_id(),
            first_name: user.get_first_name().into_owned(),
            last_name: user.get_last_name().map(|v| v.into_owned()),
            username: user.get_username().map(|v| v.into_owned()),
            is_bot: user.get_is_bot(),
        }
    }
}

#[async_trait]
impl RecordChat for Chat {
    async fn record_chat(&self) -> Result<()> {
        record_cache_chat(self).await
    }
}

#[async_trait]
impl GetUser for i64 {
    async fn get_cached_user(&self) -> Result<Option<User>> {
        get_user(*self).await
    }

    async fn cached_name(&self) -> Result<String> {
        let res = if let Some(user) = self.get_cached_user().await? {
            user.name_humanreadable()
        } else {
            self.to_string()
        };
        Ok(res)
    }

    async fn mention(&self) -> Result<Markup<String>> {
        let res = if let Some(user) = self.get_cached_user().await? {
            let name = user.name_humanreadable();
            MarkupType::TextMention(user).text(name)
        } else {
            MarkupType::Text.text(self.to_string())
        };
        Ok(res)
    }
}

#[async_trait]
impl GetUser for User {
    async fn get_cached_user(&self) -> Result<Option<User>> {
        Ok(Some(self.clone()))
    }

    async fn cached_name(&self) -> Result<String> {
        Ok(self.name_humanreadable())
    }

    async fn mention(&self) -> Result<Markup<String>> {
        let name = self.name_humanreadable();
        Ok(MarkupType::TextMention(self.clone()).text(name))
    }
}

#[async_trait]
impl GetChat for i64 {
    async fn get_chat(&self) -> Result<Option<Chat>> {
        get_chat(*self).await
    }
}

#[async_trait]
impl RecordUser for User {
    fn get_user<'a>(&'a self) -> Option<Cow<'a, User>> {
        Some(Cow::Borrowed(self))
    }

    async fn record_user(&self) -> Result<()> {
        record_cache_user(self).await
    }
}

#[async_trait]
impl RecordUser for UpdateExt {
    fn get_user<'a>(&'a self) -> Option<Cow<'a, User>> {
        match self {
            UpdateExt::Message(ref message) => message.get_from(),
            UpdateExt::EditedMessage(ref message) => message.get_from(),
            UpdateExt::EditedChannelPost(ref message) => message.get_from(),
            UpdateExt::ChannelPost(ref message) => message.get_from(),
            UpdateExt::InlineQuery(ref inlinequery) => Some(inlinequery.get_from()),
            UpdateExt::ChosenInlineResult(ref ch) => Some(ch.get_from()),
            UpdateExt::CallbackQuery(ref cb) => Some(cb.get_from()),
            UpdateExt::ShippingQuery(ref sb) => Some(sb.get_from()),
            UpdateExt::PreCheckoutQuery(ref pcb) => Some(pcb.get_from()),
            UpdateExt::Poll(_) => None,
            UpdateExt::PollAnswer(ref pollanswer) => pollanswer.get_user(),
            UpdateExt::MyChatMember(ref upd) => Some(upd.get_from()),
            UpdateExt::ChatJoinRequest(ref req) => Some(req.get_from()),
            UpdateExt::ChatMember(ref member) => Some(member.get_from()),
            UpdateExt::Invalid => None,
            UpdateExt::MessageReaction(ref reaction) => reaction.get_user(),
            UpdateExt::MessageReactionCount(_) => None,
            UpdateExt::ChatBoost(_) => None,
            UpdateExt::RemovedChatBoost(_) => None,
        }
    }

    async fn record_user(&self) -> Result<()> {
        record_cache_update(self).await
    }
}
