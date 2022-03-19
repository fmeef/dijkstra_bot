use super::Result;
use crate::util::error::BotError;
use anyhow::anyhow;
use std::ops::DerefMut;

use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use futures::Future;
use grammers_client::types::Message;
use redis::{AsyncCommands, FromRedisValue, Pipeline, RedisError, ToRedisArgs};
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

// Workaround for redis-rs's inability to support non-utf8 strings
// as single args.
pub struct RedisStr(Vec<u8>);

impl RedisStr {
    pub fn new<T: Serialize>(val: &T) -> Result<Self> {
        let bytes = rmp_serde::to_vec(val)?;
        Ok(RedisStr(bytes))
    }

    pub async fn new_async<T: Serialize + Send + 'static>(val: T) -> Result<Self> {
        tokio::spawn(async move {
            let v: Result<Self> = Ok(RedisStr(rmp_serde::to_vec(&val)?));
            v
        })
        .await?
    }
}

impl ToRedisArgs for RedisStr {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        out.write_arg(self.0.as_slice())
    }

    fn is_single_arg(&self) -> bool {
        true
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

    // atomically create a list out of multipole Serialize types
    // any previous list at this key will be overwritten
    pub async fn create_list<T, U, V>(&self, key: &T, obj: U) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + Send + Sync + 'static,
        U: Iterator<Item = V>,
    {
        self.try_pipe(|p| {
            let p = p.atomic();
            let p = p.del(key.as_ref());
            for item in obj {
                let obj = RedisStr::new(&item)?;
                let p = p.lpush(key.as_ref(), &obj);
            }
            Ok(p)
        })
        .await?;
        Ok(())
    }

    // remove and deserialize all items in a list.
    pub async fn drain_list<T, R>(&self, key: &T) -> Result<Vec<R>>
    where
        T: AsRef<str> + Send + Sync,
        R: DeserializeOwned + Send + Sync,
    {
        let mut conn = self.pool.get().await?;
        conn.lrange::<&str, Vec<Vec<u8>>>(key.as_ref(), 0, -1)
            .await?
            .into_iter()
            .map(|v| {
                let res: Result<R> = rmp_serde::from_slice(&v.as_slice()).map_err(|e| anyhow!(e));
                res
            })
            .collect()
    }

    // construct and run a redis pipeline using the provided closure
    pub async fn pipe<T, R>(&self, func: T) -> Result<R>
    where
        for<'a> T: FnOnce(&'a mut Pipeline) -> &'a mut Pipeline,
        R: FromRedisValue,
    {
        let mut pipe = redis::pipe();
        let pipe = func(&mut pipe);
        let mut conn = self.pool.get().await?;
        let res: R = pipe.query_async(conn.deref_mut()).await?;
        Ok(res)
    }

    // construct and run a redis pipeline using the provided closure
    // any Err type returned will abort without running the query
    pub async fn try_pipe<T, R>(&self, func: T) -> Result<R>
    where
        for<'a> T: FnOnce(&'a mut Pipeline) -> Result<&'a mut Pipeline>,
        R: FromRedisValue,
    {
        let mut pipe = redis::pipe();
        let pipe = func(&mut pipe)?;
        let mut conn = self.pool.get().await?;
        let res: R = pipe.query_async(conn.deref_mut()).await?;
        Ok(res)
    }

    // Run one or more redis queries using the connection provided to the
    // closure
    pub async fn query<'a, T, R, Fut>(&'a self, func: T) -> Result<R>
    where
        T: FnOnce(PooledConnection<'a, RedisConnectionManager>) -> Fut + Send,
        Fut: Future<Output = std::result::Result<R, RedisError>> + Send,
        R: Send,
    {
        Ok(func(self.pool.get().await?).await?)
    }

    // Run one or more redis queries using the connection provided to the
    // closure. The closure is run via a separate tokio task
    pub async fn query_spawn<T, R, Fut>(&self, func: T) -> JoinHandle<Result<R>>
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

    // Gets a single connection from the connection pool
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
