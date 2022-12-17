use crate::persist::{core::users::Model as UserModel, redis::RedisStr};
use crate::statics::{CONFIG, REDIS};
use anyhow::Result;
use async_trait::async_trait;
use botapi::gen_types::{UpdateExt, User};
use redis::AsyncCommands;

const USER_PREFIX: &str = "usrc";

pub(crate) fn get_user_cache_key(user: i64) -> String {
    return format!("{}:{}", USER_PREFIX, user);
}

pub(crate) async fn record_cache_user(user: &User) -> Result<()> {
    let user: UserModel = user.into();
    let key = get_user_cache_key(user.user_id);
    let st = RedisStr::new(&user)?;
    REDIS
        .pipe(|p| p.set(&key, st).expire(&key, CONFIG.cache_timeout))
        .await?;
    Ok(())
}

pub(crate) async fn record_cache_update(update: &UpdateExt) -> Result<()> {
    if let Some(user) = update.get_user() {
        record_cache_user(user).await?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn get_user(user: i64) -> Result<Option<UserModel>> {
    let key = get_user_cache_key(user);
    let model: Option<RedisStr> = REDIS.sq(|p| p.get(&key)).await?;
    Ok(model
        .map(|v| v.get::<UserModel>())
        .map_or(Ok(None), |v| v.map(Some))?)
}
#[async_trait]
pub(crate) trait GetUser {
    fn get_user<'a>(&'a self) -> Option<&'a User>;
    async fn record_user(&self) -> Result<()>;
    async fn get_cached_user(&self) -> Result<Option<UserModel>>;
}

impl From<&User> for UserModel {
    fn from(user: &User) -> Self {
        Self {
            user_id: user.get_id(),
            username: user.get_username().map(|v| v.to_owned()),
        }
    }
}

#[async_trait]
impl GetUser for User {
    fn get_user<'a>(&'a self) -> Option<&'a User> {
        Some(&self)
    }

    async fn record_user(&self) -> Result<()> {
        record_cache_user(self).await
    }

    async fn get_cached_user(&self) -> Result<Option<UserModel>> {
        get_user(self.get_id()).await
    }
}

#[async_trait]
impl GetUser for UpdateExt {
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

    async fn get_cached_user(&self) -> Result<Option<UserModel>> {
        if let Some(user) = self.get_user() {
            get_user(user.get_id()).await
        } else {
            Ok(None)
        }
    }
}
