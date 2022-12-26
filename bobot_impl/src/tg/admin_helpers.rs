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
    async fn get_cached_admins(&self) -> Result<Vec<ChatMember>>;
    async fn refresh_cached_admins(&self) -> Result<Vec<ChatMember>>;
}

#[async_trait]
impl GetCachedAdmins for Chat {
    async fn get_cached_admins(&self) -> Result<Vec<ChatMember>> {
        let key = get_chat_admin_cache_key(self.get_id());
        let admins: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
        if let Some(admins) = admins {
            Ok(admins.get::<Vec<ChatMember>>()?)
        } else {
            self.refresh_cached_admins().await
        }
    }

    async fn refresh_cached_admins(&self) -> Result<Vec<ChatMember>> {
        let admins = TG
            .client()
            .build_get_chat_administrators(self.get_id())
            .chat_id(self.get_id())
            .build()
            .await?;
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
