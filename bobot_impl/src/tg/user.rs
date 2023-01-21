use std::borrow::Cow;

use crate::persist::redis::RedisStr;
use crate::statics::{CONFIG, REDIS, TG};
use crate::util::error::Result;
use async_trait::async_trait;
use botapi::gen_types::{Chat, UpdateExt, User};
use redis::AsyncCommands;

pub fn get_user_cache_key(user: i64) -> String {
    format!("usrc:{}", user)
}

pub fn get_username_cache_key(username: &str) -> String {
    format!("uname:{}", username)
}

fn get_chat_cache_key(chat: i64) -> String {
    format!("chat:{}", chat)
}

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

pub async fn record_cache_user(user: &User) -> Result<()> {
    let key = get_user_cache_key(user.get_id());
    let st = RedisStr::new(user)?;
    if let Some(username) = user.get_username() {
        let uname = get_username_cache_key(&username);
        REDIS
            .pipe(|p| {
                p.set(&key, st)
                    .expire(&key, CONFIG.cache_timeout)
                    .set(&uname, user.get_id())
                    .expire(&uname, CONFIG.cache_timeout)
            })
            .await?;
    } else {
        REDIS
            .pipe(|p| p.set(&key, st).expire(&key, CONFIG.cache_timeout))
            .await?;
    }
    Ok(())
}

pub async fn record_cache_chat(chat: &Chat) -> Result<()> {
    let key = get_chat_cache_key(chat.get_id());
    let st = RedisStr::new(chat)?;
    REDIS
        .pipe(|p| p.set(&key, st).expire(&key, CONFIG.cache_timeout))
        .await?;
    Ok(())
}

pub async fn record_cache_update(update: &UpdateExt) -> Result<()> {
    if let Some(user) = update.get_user() {
        record_cache_user(&user).await?;
    }
    if let UpdateExt::Message(m) = update {
        if let Some(m) = m.get_reply_to_message() {
            if let Some(user) = m.get_from() {
                user.record_user().await?;
            }
            if let Some(m) = m.get_forward_from() {
                m.record_user().await?;
            }
        }
        if let Some(m) = m.get_forward_from() {
            m.record_user().await?;
        }
    }
    Ok(())
}

pub async fn get_user(user: i64) -> Result<Option<User>> {
    let key = get_user_cache_key(user);
    let model: Option<RedisStr> = REDIS.sq(|p| p.get(&key)).await?;
    if let Some(model) = model {
        Ok(Some(model.get()?))
    } else {
        Ok(None)
    }
}

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

pub async fn get_chat(chat: i64) -> Result<Option<Chat>> {
    let key = get_chat_cache_key(chat);
    let model: Option<RedisStr> = REDIS.sq(|p| p.get(&key)).await?;
    if let Some(model) = model {
        Ok(Some(model.get()?))
    } else {
        Ok(None)
    }
}

#[async_trait]
pub trait RecordChat {
    async fn record_chat(&self) -> Result<()>;
}

#[async_trait]
pub trait GetChat {
    async fn get_chat(&self) -> Result<Option<Chat>>;
}

#[async_trait]
pub trait RecordUser {
    fn get_user<'a>(&'a self) -> Option<Cow<'a, User>>;
    async fn record_user(&self) -> Result<()>;
}

#[async_trait]
pub trait GetUser {
    async fn get_cached_user(&self) -> Result<Option<User>>;
}

impl From<&User> for crate::persist::core::users::Model {
    fn from(user: &User) -> Self {
        Self {
            user_id: user.get_id(),
            username: user.get_username().map(|v| v.into_owned()),
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
            UpdateExt::PollAnswer(ref pollanswer) => Some(pollanswer.get_user()),
            UpdateExt::MyChatMember(ref upd) => Some(upd.get_from()),
            UpdateExt::ChatJoinRequest(ref req) => Some(req.get_from()),
            UpdateExt::ChatMember(ref member) => Some(member.get_from()),
            UpdateExt::Invalid => None,
        }
    }

    async fn record_user(&self) -> Result<()> {
        record_cache_update(self).await
    }
}
