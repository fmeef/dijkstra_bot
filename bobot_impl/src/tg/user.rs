use crate::persist::{core::users::Model as UserModel, redis::RedisStr};
use crate::statics::{REDIS, CONFIG};
use anyhow::Result;
use botapi::gen_types::{UpdateExt, User};

const USER_PREFIX: &str = "usrc";

pub(crate) fn get_user_cache_key(user: i64) -> String {
    return format!("{}:{}", USER_PREFIX, user);
}

pub(crate) async fn record_user(update: &UpdateExt) -> Result<()> {
    if let Some(user) = update.get_user() {
        let user: UserModel = user.into();
        let key = get_user_cache_key(user.user_id);
        let st = RedisStr::new_async(user).await?;
        REDIS.pipe(|p| p.set(&key, st).expire(&key, CONFIG.cache_timeout)).await?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn get_user(user: i64) -> Result<UserModel> { 
    let key = get_user_cache_key(user);
    let model: RedisStr = REDIS.pipe(|p| p.get(&key)).await?;
    Ok(model.get::<UserModel>()?)
}

pub(crate) trait GetUser {
    fn get_user<'a>(&'a self) -> Option<&'a User>;
}

impl From<&User> for UserModel {
    fn from(user: &User) -> Self {
        Self {
            user_id: user.get_id(),
            username: user.get_username().map(|v| v.to_owned()),
        }
    }
}

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
}
