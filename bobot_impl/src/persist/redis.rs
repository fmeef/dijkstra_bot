use super::Result;
use crate::util::{
    callback::{
        BotDbFuture, BoxedCacheCallback, CacheCallback, CacheCb, CacheMissCallback, CacheMissCb,
        OutputBoxer,
    },
    error::BotError,
};
use anyhow::anyhow;
use sea_orm::DatabaseConnection;
use std::ops::DerefMut;

use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use async_trait::async_trait;
use futures::Future;
use higher_order_closure::higher_order_closure;
use redis::{AsyncCommands, ErrorKind, FromRedisValue, Pipeline, RedisError, ToRedisArgs};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use teloxide::types::Message;
use tokio::task::JoinHandle;
use uuid::Uuid;

// write cache redis keys
pub const KEY_WRITE_CACHE: &str = "writecache";
pub const KEY_TYPE_PREFIX: &str = "wc:typeprefix";
pub const KEY_WRAPPER: &str = "wc:wrapper";
pub const KEY_TYPE_VAL: &str = "wc:typeval";
pub const KEY_UUID: &str = "wc:uuid";

pub(crate) struct CachedQuery<R> {
    redis_query: CacheCb<RedisPool, R>,
    sql_query: CacheCb<Arc<DatabaseConnection>, R>,
    miss_query: CacheMissCb<RedisPool, R>,
}

pub(crate) struct CachedQueryBuilder<R> {
    redis_query: Option<CacheCb<RedisPool, R>>,
    sql_query: Option<CacheCb<Arc<DatabaseConnection>, R>>,
    miss_query: Option<CacheMissCb<RedisPool, R>>,
}

impl<R> CachedQueryBuilder<R> {
    pub(crate) fn redis_query<'a, F>(mut self, func: F) -> Self
    where
        F: for<'b> CacheCallback<'b, RedisPool, R> + 'static,
    {
        //       self.redis_query = Some(CacheCb::new(func));
        self
    }

    pub(crate) fn sql_query<'a, F, Fut>(mut self, func: F) -> Self
    where
        F: for<'b> FnOnce(&'b String, &'b Arc<DatabaseConnection>) -> Fut + Sync + Send + 'a,
        R: DeserializeOwned + 'a,
        Fut: Future<Output = Result<Option<R>>> + Send + 'a,
    {
        //self.sql_query = Some(CacheCb(Box::new(OutputBoxer(func))));
        self
    }

    pub(crate) fn miss_query<F, Fut>(mut self, func: F) -> Self
    where
        F: for<'b> FnOnce(&'b String, &'b R, &'b RedisPool) -> Fut + Sync + Send + 'static,
        R: Serialize + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let b = Box::new(OutputBoxer(func));
        self.miss_query = Some(CacheMissCb(b));
        self
    }

    pub(crate) fn build(self) -> Result<CachedQuery<R>> {
        if let (Some(sql_query), Some(redis_query), Some(miss_query)) =
            (self.sql_query, self.redis_query, self.miss_query)
        {
            Ok(CachedQuery {
                sql_query,
                redis_query,
                miss_query,
            })
        } else {
            Err(anyhow!(BotError::new("failed to build CachedQuery")))
        }
    }
}

#[async_trait]
pub(crate) trait CachedQueryTrait<R>
where
    R: DeserializeOwned + 'static,
{
    async fn query(
        self,
        db: Arc<DatabaseConnection>,
        redis: RedisPool,
        key: String,
    ) -> Result<Option<R>>;
}

impl<R> CachedQuery<R>
where
    R: DeserializeOwned,
{
    pub(crate) fn builder() -> CachedQueryBuilder<R> {
        CachedQueryBuilder {
            redis_query: None,
            sql_query: None,
            miss_query: None,
        }
    }
}

#[async_trait]
impl<R> CachedQueryTrait<R> for CachedQuery<R>
where
    R: DeserializeOwned + Serialize + Clone + Send + Sync + 'static,
{
    async fn query(
        self,
        db: Arc<DatabaseConnection>,
        redis: RedisPool,
        key: String,
    ) -> Result<Option<R>> {
        if let Some(val) = self.redis_query.cb(&key, &redis).await? {
            Ok(Some(val))
        } else {
            let val = if let Some(val) = self.sql_query.cb(&key, &db).await? {
                self.miss_query.cb(&key, &val, &redis).await?;
                Some(val)
            } else {
                None
            };
            Ok(val)
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
pub fn random_key<T: AsRef<str>>(prefix: &T) -> String {
    let uuid = Uuid::new_v4();
    format!("r:{}:{}", prefix.as_ref(), uuid.to_string())
}

#[inline(always)]
pub fn scope_key_by_user<T: AsRef<str>>(key: &T, user: i64) -> String {
    format!("u:{}:{}", user, key.as_ref())
}

#[inline(always)]
pub fn scope_key<T: AsRef<str>>(key: &T, message: &Message, prefix: &str) -> Result<String> {
    let user_id = message
        .from()
        .ok_or_else(|| BotError::new("message without sender"))?
        .id;
    let chat_id = message.chat.id;
    let res = format!("{}:{}:{}:{}", prefix, chat_id, user_id, key.as_ref());
    Ok(res)
}

#[inline(always)]
pub fn scope_key_by_chatuser<T: AsRef<str>>(key: &T, message: &Message) -> Result<String> {
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
                p.lpush(key.as_ref(), &obj);
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
