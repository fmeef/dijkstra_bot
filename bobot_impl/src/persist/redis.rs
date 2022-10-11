use super::Result;
use crate::util::{
    callback::{CacheCallback, CacheMissCallback},
    error::BotError,
};
use anyhow::anyhow;
use sea_orm::DatabaseConnection;
use std::{marker::PhantomData, ops::DerefMut};

use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use async_trait::async_trait;
use futures::Future;

use redis::{AsyncCommands, ErrorKind, FromRedisValue, Pipeline, RedisError, ToRedisArgs};
use serde::{de::DeserializeOwned, Serialize};

use teloxide::types::Message;
use tokio::task::JoinHandle;
use uuid::Uuid;

// write cache redis keys
pub const KEY_WRITE_CACHE: &str = "writecache";
pub const KEY_TYPE_PREFIX: &str = "wc:typeprefix";
pub const KEY_WRAPPER: &str = "wc:wrapper";
pub const KEY_TYPE_VAL: &str = "wc:typeval";
pub const KEY_UUID: &str = "wc:uuid";

async fn redis_query_vec<'a, R>(key: &'a str, redis: &'a RedisPool) -> Result<Option<Vec<R>>>
where
    R: DeserializeOwned + DeserializeOwned + Sync + Send + 'a,
{
    let res: Vec<R> = redis.drain_list(key).await?;
    if res.len() == 0 {
        Ok(None)
    } else {
        Ok(Some(res))
    }
}

async fn redis_miss_vec<'a, V>(key: &'a str, val: Vec<V>, redis: &'a RedisPool) -> Result<Vec<V>>
where
    V: Serialize + DeserializeOwned + Send + Sync + 'a,
{
    redis.create_list(key, val.iter()).await?;
    Ok(val)
}

async fn redis_query<'a, R>(key: &'a str, redis: &'a RedisPool) -> Result<Option<R>>
where
    R: DeserializeOwned + Sync + Send + 'a,
{
    let res: Option<RedisStr> = redis
        .query(|mut c| async move {
            if !c.exists(key).await? {
                Ok(None)
            } else {
                Ok(Some(c.get(key).await?))
            }
        })
        .await?;

    let res = res.map(|v| v.get::<R>().ok()).flatten();
    Ok(res)
}

async fn redis_miss<'a, V>(key: &'a str, val: V, redis: &'a RedisPool) -> Result<V>
where
    V: Serialize + 'a,
{
    let valstr = RedisStr::new(&val)?;
    redis.pipe(|p| p.set(key, valstr)).await?;
    Ok(val)
}

/*
 * Helper type for caching a single value from the database
 * in redis.
 */
pub(crate) struct CachedQuery<'r, T, R, S, M>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    R: CacheCallback<'r, RedisPool, T> + Send + Sync,
    S: CacheCallback<'r, DatabaseConnection, T> + Send + Sync,
    M: CacheMissCallback<'r, RedisPool, T> + Send + Sync,
{
    redis_query: R,
    sql_query: S,
    miss_query: M,
    phantom: PhantomData<&'r T>,
}

#[async_trait]
pub(crate) trait CachedQueryTrait<'r, R>
where
    R: DeserializeOwned,
{
    async fn query(
        self,
        db: &'r DatabaseConnection,
        redis: &'r RedisPool,
        key: &'r str,
    ) -> Result<Option<R>>;
}

pub(crate) fn default_cached_query_vec<'r, T, S>(sql_query: S) -> impl CachedQueryTrait<'r, Vec<T>>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'r,
    S: CacheCallback<'r, DatabaseConnection, Vec<T>> + Send + Sync,
{
    CachedQuery::new(sql_query, redis_query_vec, redis_miss_vec)
}

/*
 * Return a default cached query that stores cached values as
 * single redis key. This behavior can be overridden if more
 * complex redis structures are required
 */
pub(crate) fn default_cache_query<'r, T, S>(sql_query: S) -> impl CachedQueryTrait<'r, T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'r,
    S: CacheCallback<'r, DatabaseConnection, T> + Send + Sync,
{
    CachedQuery::new(sql_query, redis_query, redis_miss)
}

impl<'r, T, R, S, M> CachedQuery<'r, T, R, S, M>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    R: CacheCallback<'r, RedisPool, T> + Send + Sync,
    S: CacheCallback<'r, DatabaseConnection, T> + Send + Sync,
    M: CacheMissCallback<'r, RedisPool, T> + Send + Sync,
{
    pub(crate) fn new(sql_query: S, redis_query: R, miss_query: M) -> Self {
        Self {
            redis_query,
            sql_query,
            miss_query,
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<'r, T, R, S, M> CachedQueryTrait<'r, T> for CachedQuery<'r, T, R, S, M>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    R: CacheCallback<'r, RedisPool, T> + Send + Sync,
    S: CacheCallback<'r, DatabaseConnection, T> + Send + Sync,
    M: CacheMissCallback<'r, RedisPool, T> + Send + Sync,
{
    async fn query(
        self,
        db: &'r DatabaseConnection,
        redis: &'r RedisPool,
        key: &'r str,
    ) -> Result<Option<T>> {
        if let Some(val) = self.redis_query.cb(key, redis).await? {
            Ok(Some(val))
        } else {
            let val = self.sql_query.cb(key, db).await?;
            if let Some(val) = val {
                Ok(Some(self.miss_query.cb(key, val, redis).await?))
            } else {
                Ok(None)
            }
        }
    }
}

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

    pub fn get<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let res: T = rmp_serde::from_read(self.0.as_slice())?;
        Ok(res)
    }
}

impl FromRedisValue for RedisStr {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match *v {
            redis::Value::Data(ref data) => Ok(RedisStr(data.to_owned())),
            _ => Err(RedisError::from((
                ErrorKind::TypeError,
                "Response was of incompatible type",
                format!("{:?} (response was {:?})", "Invalid RedisStr", v),
            ))),
        }
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

#[inline(always)]
pub fn random_key(prefix: &str) -> String {
    let uuid = Uuid::new_v4();
    format!("r:{}:{}", prefix, uuid.to_string())
}

#[inline(always)]
pub fn scope_key_by_user(key: &str, user: i64) -> String {
    format!("u:{}:{}", user, key)
}

#[inline(always)]
pub fn scope_key(key: &str, message: &Message, prefix: &str) -> Result<String> {
    let user_id = message
        .from()
        .ok_or_else(|| BotError::new("message without sender"))?
        .id;
    let chat_id = message.chat.id;
    let res = format!("{}:{}:{}:{}", prefix, chat_id, user_id, key);
    Ok(res)
}

#[inline(always)]
pub fn scope_key_by_chatuser(key: &str, message: &Message) -> Result<String> {
    scope_key(key, message, "cu")
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
    pub async fn create_list<U, V>(&self, key: &str, mut obj: U) -> Result<()>
    where
        V: Serialize + Send + Sync,
        U: Iterator<Item = V>,
    {
        self.try_pipe(|p| {
            p.atomic();
            p.del(key);
            obj.try_for_each(|v| {
                let v = RedisStr::new(&v)?;
                p.lpush(key, v);
                Ok::<(), anyhow::Error>(())
            })?;
            Ok(p)
        })
        .await?;
        Ok(())
    }

    // remove and deserialize all items in a list.
    pub async fn drain_list<R>(&self, key: &str) -> Result<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync,
    {
        let mut conn = self.pool.get().await?;
        conn.lrange::<&str, Vec<Vec<u8>>>(key, 0, -1)
            .await?
            .into_iter()
            .map(|v| rmp_serde::from_slice(&v.as_slice()).map_err(|e| anyhow!(e)))
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
