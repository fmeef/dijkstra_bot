use std::{borrow::BorrowMut, ops::DerefMut};

use super::Result;
use crate::util::error::BotError;

use async_trait::async_trait;
use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use anyhow::anyhow;
use futures::Future;
use grammers_client::types::Message;
use lazy_static::__Deref;
use redis::{aio::Connection, AsyncCommands, RedisError, ToRedisArgs};
use serde::{de::DeserializeOwned, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

// write cache redis keys
pub const KEY_WRITE_CACHE: &str = "writecache";
pub const KEY_TYPE_PREFIX: &str = "wc:typeprefix";
pub const KEY_WRAPPER: &str = "wc:wrapper";
pub const KEY_TYPE_VAL: &str = "wc:typeval";
pub const KEY_UUID: &str = "wc:uuid";

pub fn error_mapper(err: RedisError) -> BotError {
    match err.kind() {
        _ => BotError::new("some redis error"),
    }
}

pub fn random_key<T: AsRef<str>>(prefix: &T) -> String {
    let uuid = Uuid::new_v4();
    format!("r:{}:{}", prefix.as_ref(), uuid.to_string())
}

pub fn scope_key_by_user<T: AsRef<str>>(key: &T, user: i64) -> String {
    format!("u:{}:{}", user, key.as_ref())
}

pub fn scope_key_by_chatuser<T: AsRef<str>>(key: &T, message: &Message) -> Result<String> {
    let user_id = message
        .sender()
        .ok_or_else(|| BotError::new("message without sender"))?
        .id();
    let chat_id = message.chat().id();
    let res = format!("cu:{}:{}:{}", chat_id, user_id, key.as_ref());
    Ok(res)
}

pub struct RedisPoolBuilder {
    connectionstr: String,
}

pub struct RedisPool {
    pool: Pool<RedisConnectionManager>,
}

impl RedisPoolBuilder {
    pub fn new<T: ToString>(connectonstr: T) -> Self {
        RedisPoolBuilder {
            connectionstr: connectonstr.to_string(),
        }
    }

    pub async fn build(self) -> Result<RedisPool> {
        RedisPool::new(self.connectionstr).await
    }
}

impl RedisPool {
    pub async fn new<T: AsRef<str>>(connectionstr: T) -> Result<Self> {
        let client = RedisConnectionManager::new(connectionstr.as_ref())?;

        let pool = Pool::builder().max_size(15).build(client).await?;
        Ok(RedisPool { pool })
    }

    pub async fn create_list<T, U, V>(&self, key: &T, obj: U) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
        U: Iterator<Item = V>,
    {
        let mut conn = self.pool.get().await?;
        let mut p = redis::pipe();
        let p = p.atomic();
        let p = p.del(key.as_ref());
        for item in obj {
            let obj: Vec<u8> = rmp_serde::to_vec(&item)?;
            let p = p.lpush(key.as_ref(), &obj);
        }
        p.query_async(conn.deref_mut()).await?;
        Ok(())
    }

    pub async fn redis_query<'a, T, R, Fut>(&'a self, func: T) -> Result<R>
    where
        T: FnOnce(PooledConnection<'a, RedisConnectionManager>) -> Fut + Send,
        Fut: Future<Output = std::result::Result<R, RedisError>> + Send,
        R: Send,
    {
        Ok(func(self.pool.get().await?).await?)
    }

    pub async fn redis_query_spawn<T, R, Fut>(&self, func: T) -> JoinHandle<Result<R>>
    where
        T: for<'b> FnOnce(PooledConnection<'b, RedisConnectionManager>) -> Fut + Send + 'static,
        Fut: Future<Output = std::result::Result<R, RedisError>> + Send,
        R: Send + 'static,
    {
        let r = self.clone();
        tokio::spawn(async move {
            let res = func(r.pool.get().await?).await?;
            let res: Result<R> = Ok(res);
            res
        })
    }

    pub async fn conn<'a>(&'a self) -> Result<PooledConnection<'a, RedisConnectionManager>> {
        let res = self.pool.get().await?;
        Ok(res)
    }
}

impl Clone for RedisPool {
    fn clone(&self) -> Self {
        RedisPool {
            pool: self.pool.clone(),
        }
    }
}
