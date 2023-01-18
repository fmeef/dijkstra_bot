use crate::persist::redis::RedisStr;
use crate::statics::{CONFIG, REDIS};
use anyhow::Result;
use async_trait::async_trait;
use botapi::gen_types::{Chat, UpdateExt, User};
use redis::AsyncCommands;

const USER_PREFIX: &str = "usrc";

pub fn get_user_cache_key(user: i64) -> String {
    format!("{}:{}", USER_PREFIX, user)
}

fn get_chat_cache_key(chat: i64) -> String {
    format!("chat:{}", chat)
}

pub async fn record_cache_user(user: &User) -> Result<()> {
    let key = get_user_cache_key(user.get_id());
    let st = RedisStr::new(user)?;
    REDIS
        .pipe(|p| p.set(&key, st).expire(&key, CONFIG.cache_timeout))
        .await?;
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
        record_cache_user(user).await?;
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
    fn get_user<'a>(&'a self) -> Option<&'a User>;
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
            username: user.get_username().map(|v| v.to_owned()),
        }
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
impl RecordChat for Chat {
    async fn record_chat(&self) -> Result<()> {
        record_cache_chat(self).await
    }
}

#[async_trait]
impl RecordUser for User {
    fn get_user<'a>(&'a self) -> Option<&'a User> {
        Some(&self)
    }

    async fn record_user(&self) -> Result<()> {
        record_cache_user(self).await
    }
}

#[async_trait]
impl RecordUser for UpdateExt {
    fn get_user<'a>(&'a self) -> Option<&'a User> {
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
