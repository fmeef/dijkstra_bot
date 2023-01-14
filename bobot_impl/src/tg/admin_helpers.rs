use std::collections::HashMap;

use crate::{
    persist::redis::RedisStr,
    statics::{REDIS, TG},
};
use anyhow::Result;
use async_trait::async_trait;
use botapi::gen_types::{Chat, ChatMember, User};
use chrono::Duration;
use redis::AsyncCommands;

static CACHE_ME: &str = "getme";

pub(crate) async fn get_me() -> Result<User> {
    let st: Option<RedisStr> = REDIS.sq(|q| q.get(&CACHE_ME)).await?;
    if let Some(st) = st {
        st.get::<User>()
    } else {
        let user = TG.client().get_me().await?;
        let u = RedisStr::new(&user)?;
        REDIS
            .pipe(|p| {
                p.set(&CACHE_ME, u)
                    .expire(&CACHE_ME, Duration::minutes(10).num_seconds() as usize)
            })
            .await?;
        Ok(user)
    }
}

fn get_chat_admin_cache_key(chat: i64) -> String {
    format!("ca:{}", chat)
}

#[async_trait]
pub(crate) trait GetCachedAdmins {
    async fn get_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn is_admin(&self, user: i64) -> Result<Option<ChatMember>>;
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
                    Ok::<_, anyhow::Error>(acc)
                })?;
            Ok(admins)
        } else {
            self.refresh_cached_admins().await
        }
    }

    async fn is_admin(&self, user: i64) -> Result<Option<ChatMember>> {
        let key = get_chat_admin_cache_key(self.get_id());
        let admin: Option<RedisStr> = REDIS.sq(|q| q.hget(&key, user)).await?;
        if let Some(user) = admin {
            Ok(Some(user.get::<ChatMember>()?))
        } else {
            Ok(None)
        }
    }

    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>> {
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
        let key = get_chat_admin_cache_key(self.get_id());

        REDIS
            .try_pipe(|q| {
                admins.try_for_each(|(id, cm)| {
                    q.hset(&key, id, RedisStr::new(&cm)?);
                    Ok::<(), anyhow::Error>(())
                })?;
                Ok(q.expire(&key, Duration::hours(48).num_seconds() as usize))
            })
            .await?;
        Ok(res)
    }
}
