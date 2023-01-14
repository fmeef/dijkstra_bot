use std::collections::HashMap;

use crate::{
    persist::redis::RedisStr,
    statics::{REDIS, TG},
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use botapi::gen_types::{Chat, ChatMember, User};
use chrono::Duration;
use redis::AsyncCommands;

use super::user::GetUser;

static CACHE_ME: &str = "getme";

pub async fn is_self_admin(chat: &Chat) -> Result<bool> {
    let me = get_me().await?;
    Ok(chat.is_user_admin(me.get_id()).await?.is_some())
}

pub async fn self_admin_or_die(chat: &Chat) -> Result<()> {
    if !is_self_admin(chat).await? {
        TG.client()
            .build_send_message(
                chat.get_id(),
                "I'm sorry, Dave. I need to be admin to function in this group",
            )
            .build()
            .await?;
        Err(anyhow!("not admin"))
    } else {
        Ok(())
    }
}

pub async fn get_me() -> Result<User> {
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
pub trait IsAdmin {
    async fn is_admin(&self, chat: &Chat) -> Result<bool>;
    async fn admin_or_die(&self, chat: &Chat) -> Result<()>;
}

#[async_trait]
pub trait GetCachedAdmins {
    async fn get_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn refresh_cached_admins(&self) -> Result<HashMap<i64, ChatMember>>;
    async fn is_user_admin(&self, user: i64) -> Result<Option<ChatMember>>;
}

#[async_trait]
impl IsAdmin for User {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        Ok(chat.is_user_admin(self.get_id()).await?.is_some())
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if self.is_admin(chat).await? {
            Ok(())
        } else {
            let msg = format!(
                "User {} lacking admin rights",
                self.get_username()
                    .unwrap_or(self.get_id().to_string().as_str())
            );
            TG.client()
                .build_send_message(chat.get_id(), &msg)
                .build()
                .await?;
            Err(anyhow!("user is not admin"))
        }
    }
}

#[async_trait]
impl IsAdmin for Option<&User> {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        if let Some(user) = self {
            Ok(chat.is_user_admin(user.get_id()).await?.is_some())
        } else {
            Ok(false)
        }
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if let Some(user) = self {
            if user.is_admin(chat).await? {
                Ok(())
            } else {
                let msg = format!(
                    "User {} lacking admin rights",
                    user.get_username()
                        .unwrap_or(user.get_id().to_string().as_str())
                );
                TG.client()
                    .build_send_message(chat.get_id(), &msg)
                    .build()
                    .await?;
                Err(anyhow!("user is not admin"))
            }
        } else {
            Err(anyhow!("invalid user"))
        }
    }
}

#[async_trait]
impl IsAdmin for i64 {
    async fn is_admin(&self, chat: &Chat) -> Result<bool> {
        Ok(chat.is_user_admin(*self).await?.is_some())
    }

    async fn admin_or_die(&self, chat: &Chat) -> Result<()> {
        if self.is_admin(chat).await? {
            Ok(())
        } else {
            let msg = if let Some(user) = self.get_cached_user().await? {
                format!(
                    "User {} lacking admin rights",
                    user.get_username().unwrap_or(self.to_string().as_str())
                )
            } else {
                format!("User {} lacking admin rights", self)
            };
            TG.client()
                .build_send_message(chat.get_id(), &msg)
                .build()
                .await?;
            Err(anyhow!("user is not admin"))
        }
    }
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

    async fn is_user_admin(&self, user: i64) -> Result<Option<ChatMember>> {
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
