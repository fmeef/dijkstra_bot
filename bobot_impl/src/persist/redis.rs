use super::Result;
use crate::util::error::BotError;

use async_trait::async_trait;
use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use futures::Future;
use grammers_client::types::Message;
use redis::{aio::Connection, AsyncCommands, RedisError};
use serde::{de::DeserializeOwned, Serialize};
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

#[async_trait]
trait ConnectionExt {
    async fn create_obj<T, V>(&mut self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static;

    async fn create_obj_expire<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static;

    async fn create_obj_expire_at<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static;

    async fn get_type_name<T>(&mut self, key: &T) -> Result<String>
    where
        T: AsRef<str> + Send + Sync + 'static;
    async fn get_type_data<T>(&mut self, key: &T) -> Result<Vec<u8>>
    where
        T: AsRef<str> + Send + Sync + 'static;
}

pub struct RedisPoolBuilder {
    connectionstr: String,
}

pub struct RedisPool {
    pool: Pool<RedisConnectionManager>,
}

#[async_trait]
impl ConnectionExt for Connection {
    async fn create_obj<T, V>(&mut self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_VAL, obj)
            .query_async(self)
            .await?;
        Ok(())
    }

    async fn create_obj_expire<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_VAL, obj)
            .expire(key.as_ref(), seconds)
            .query_async(self)
            .await?;
        Ok(())
    }

    async fn create_obj_expire_at<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_VAL, obj)
            .expire_at(key.as_ref(), seconds)
            .query_async(self)
            .await?;
        Ok(())
    }

    async fn get_type_name<T>(&mut self, key: &T) -> Result<String>
    where
        T: AsRef<str> + Send + Sync + 'static,
    {
        let t: String = self.hget(key.as_ref(), KEY_TYPE_PREFIX).await?;
        Ok(t)
    }

    async fn get_type_data<T>(&mut self, key: &T) -> Result<Vec<u8>>
    where
        T: AsRef<str> + Send + Sync + 'static,
    {
        let t: Vec<u8> = self.hget(key.as_ref(), KEY_TYPE_VAL).await?;
        Ok(t)
    }
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

    pub async fn clear_writecache(&self) -> Result<()> {
        let mut conn = self.pool.get().await?;

        conn.del(KEY_WRITE_CACHE).await?;
        Ok(())
    }

    async fn get_cache_size(&self) -> Result<usize> {
        let mut conn = self.pool.get().await?;

        let size: usize = conn.llen(KEY_WRITE_CACHE).await?;

        Ok(size)
    }

    pub async fn get_obj<'a, T, V>(&self, key: &'a T) -> Result<V>
    where
        T: AsRef<str> + Sync + Send + 'static,
        V: DeserializeOwned + Sync + Send + 'static,
    {
        let mut conn = self.pool.get().await?;
        let d = conn.get_type_data(key).await?;
        let obj: V = rmp_serde::from_read_ref(d.as_slice())?;
        Ok(obj)
    }

    pub async fn create_obj<T, V>(&self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        conn.create_obj(key, obj).await
    }

    pub async fn create_obj_auto<V>(&self, obj: &V) -> Result<String>
    where
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj(&key, obj).await?;
        Ok(key)
    }

    pub async fn create_obj_expire<T, V>(&self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        conn.create_obj_expire(key, obj, seconds).await?;
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

    pub async fn create_obj_expire_at<T, V>(&self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        conn.create_obj_expire_at(key, obj, seconds).await?;
        Ok(())
    }

    pub async fn create_obj_expire_auto<V>(&self, obj: &V, seconds: usize) -> Result<String>
    where
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj_expire(&key, obj, seconds).await?;
        Ok(key)
    }

    pub async fn create_obj_expire_at_auto<V>(&self, obj: &V, seconds: usize) -> Result<String>
    where
        V: Serialize + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj_expire_at(&key, obj, seconds).await?;
        Ok(key)
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
