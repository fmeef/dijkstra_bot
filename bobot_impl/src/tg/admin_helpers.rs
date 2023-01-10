use std::collections::HashMap;

use crate::{
    persist::redis::RedisStr,
    statics::{REDIS, TG},
};
use anyhow::Result;
use async_trait::async_trait;
use botapi::gen_types::{Chat, ChatMember};
use chrono::Duration;
use redis::AsyncCommands;

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
        let admins: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
        if let Some(admins) = admins {
            Ok(admins.get::<HashMap<i64, ChatMember>>()?)
        } else {
            self.refresh_cached_admins().await
        }
    }

    async fn is_admin(&self, user: i64) -> Result<Option<ChatMember>> {
        let mut admins = self.get_cached_admins().await?;
        Ok(admins.remove(&user))
    }

    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>> {
        let admins = TG
            .client()
            .build_get_chat_administrators(self.get_id())
            .chat_id(self.get_id())
            .build()
            .await?;
        let admins = admins
            .into_iter()
            .map(|cm| (cm.get_user().get_id(), cm))
            .collect::<HashMap<i64, ChatMember>>();
        let key = get_chat_admin_cache_key(self.get_id());

        let st = RedisStr::new(&admins)?;
        REDIS
            .pipe(|q| {
                q.set(&key, st)
                    .expire(&key, Duration::hours(48).num_seconds() as usize)
            })
            .await?;
        Ok(admins)
    }
}
