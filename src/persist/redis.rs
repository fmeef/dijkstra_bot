//! Wrapper for a redis connection pool, handling connection management, serialization,
//! deserization, and sql query caching.
//!
//! Newer version of redis do use multiple threads for some operations, so using
//! a connection pool has performance benefits. This wrapper makes it easy to
//! accquire and release a pool connection for either a single command, a series of piped commands
//! or a more complex async operation containing multiple commands.
//!
//! also by default the rust `redis` crate makes it tricky to store binary data in a single key,
//! which makes serializing keys with msgpack hard. This crate contains a workaround for this that

use crate::{
    statics::CONFIG,
    util::{
        callback::{CacheCallback, CacheMissCallback},
        error::{BotError, Result},
    },
};
use chrono::Duration;
use sea_orm::{ActiveModelTrait, IntoActiveModel};

use std::{marker::PhantomData, ops::DerefMut};

use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;

use async_trait::async_trait;
use futures::Future;

use crate::statics::REDIS;
use ::redis::{
    AsyncCommands, ErrorKind, FromRedisValue, Pipeline, RedisError, RedisFuture, ToRedisArgs,
};
use botapi::gen_types::Message;
use serde::{de::DeserializeOwned, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;

// write cache redis keys
pub const KEY_WRITE_CACHE: &str = "writecache";
pub const KEY_TYPE_PREFIX: &str = "wc:typeprefix";
pub const KEY_WRAPPER: &str = "wc:wrapper";
pub const KEY_TYPE_VAL: &str = "wc:typeval";
pub const KEY_UUID: &str = "wc:uuid";

/// helper function for getting a list of deserialized values from redis
async fn redis_query_vec<'a, R, P>(key: &'a str, _: &'a P) -> Result<(bool, Vec<R>)>
where
    R: DeserializeOwned + DeserializeOwned + Sync + Send + 'a,
    P: Send + Sync,
{
    let res: Vec<R> = REDIS.drain_list(key).await?;
    if res.len() == 0 {
        Ok((false, vec![]))
    } else {
        Ok((true, res))
    }
}

/// default sql caching miss operation for a list of values
async fn redis_miss_vec<'a, V>(key: &'a str, val: Vec<V>) -> Result<Vec<V>>
where
    V: Serialize + DeserializeOwned + Send + Sync + 'a,
{
    REDIS.create_list(key, val.iter()).await?;
    Ok(val)
}

/// default sql query caching query operation
pub async fn redis_query<'a, R, P>(key: &'a str, _: &'a P) -> Result<(bool, Option<R>)>
where
    R: DeserializeOwned + Sync + Send + 'a,
    P: Send + Sync + 'a,
{
    let res: Option<RedisStr> = REDIS.sq(|q| q.get(key)).await?;
    if let Some(res) = res {
        Ok((true, res.get()?))
    } else {
        Ok((false, None))
    }
}

/// Default sql query cachin miss operation for a single value
pub async fn redis_miss<'a, V>(key: &'a str, val: Option<V>, expire: Duration) -> Result<Option<V>>
where
    V: Serialize + 'a,
{
    let valstr = RedisStr::new(&val)?;
    REDIS
        .pipe(|p| {
            p.set(key, valstr)
                .expire(key, expire.num_seconds() as usize)
        })
        .await?;
    Ok(val)
}

/// Helper type for caching a single value from the database
/// in redis. NOTE: for this to work any insert operations need to also
/// be cached using the RedisCache trait. Skipping this step will result
/// in data inconsistencies
pub struct CachedQuery<'r, T, R, S, M, P>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    P: Send + Sync + 'r,
    R: CacheCallback<'r, (bool, T), P> + Send + Sync,
    S: CacheCallback<'r, T, P> + Send + Sync,
    M: CacheMissCallback<'r, T> + Send + Sync,
{
    redis_query: R,
    sql_query: S,
    miss_query: M,
    phantom: PhantomData<&'r T>,
    phantom2: PhantomData<&'r P>,
}

/// Trait to allow returning generic cached queries containing compile-time closures
#[async_trait]
pub trait CachedQueryTrait<'r, R, P>
where
    R: DeserializeOwned,
    P: Send + Sync + 'r,
{
    async fn query(self, key: &'r str, param: &'r P) -> Result<R>;
}

/// Helper function to generate a cached query for a list of values with default behavior
pub fn default_cached_query_vec<'r, T, S, P>(sql_query: S) -> impl CachedQueryTrait<'r, Vec<T>, P>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'r,
    P: Send + Sync + 'r,
    S: CacheCallback<'r, Vec<T>, P> + Send + Sync,
{
    CachedQuery::new(sql_query, redis_query_vec, redis_miss_vec)
}

/// Return a default cached query that stores cached values as
/// single redis key. This behavior can be overridden if more
/// complex redis structures are required
pub fn default_cache_query<'r, T, S, P>(
    sql_query: S,
    expire: Duration,
) -> impl CachedQueryTrait<'r, Option<T>, P>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'r,
    P: Send + Sync + 'r,
    S: CacheCallback<'r, Option<T>, P> + Send + Sync,
{
    CachedQuery::new(sql_query, redis_query, move |key, val| async move {
        redis_miss(key, val, expire).await
    })
}

impl<'r, T, R, S, M, P> CachedQuery<'r, T, R, S, M, P>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    P: Send + Sync + 'r,
    R: CacheCallback<'r, (bool, T), P> + Send + Sync,
    S: CacheCallback<'r, T, P> + Send + Sync,
    M: CacheMissCallback<'r, T> + Send + Sync,
{
    pub fn new(sql_query: S, redis_query: R, miss_query: M) -> Self {
        Self {
            redis_query,
            sql_query,
            miss_query,
            phantom: PhantomData,
            phantom2: PhantomData,
        }
    }
}

#[async_trait]
impl<'r, T, R, S, M, P> CachedQueryTrait<'r, T, P> for CachedQuery<'r, T, R, S, M, P>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    P: Send + Sync,
    R: CacheCallback<'r, (bool, T), P> + Send + Sync,
    S: CacheCallback<'r, T, P> + Send + Sync,
    M: CacheMissCallback<'r, T> + Send + Sync,
{
    async fn query(self, key: &'r str, param: &'r P) -> Result<T> {
        let (hit, val) = self.redis_query.cb(key, param).await?;
        if hit {
            Ok(val)
        } else {
            let val = self.sql_query.cb(key, param).await?;
            Ok(self.miss_query.cb(key, val).await?)
        }
    }
}

/// Maps redis errors to types we support
pub fn error_mapper(err: RedisError) -> BotError {
    match err.kind() {
        _ => BotError::conversation_err("some redis error"),
    }
}

// Workaround for redis-rs's inability to support non-utf8 strings
// as single args. Serializes binary strings using msgpack for efficiency
pub struct RedisStr(Vec<u8>);

/// helper trait for converting types into RedisStr
pub trait ToRedisStr {
    fn to_redis(&self) -> Result<RedisStr>;
}

impl<T> ToRedisStr for T
where
    T: Serialize,
{
    fn to_redis(&self) -> Result<RedisStr> {
        RedisStr::new(&self)
    }
}

impl RedisStr {
    /// Create a new RedisStr from a serializable value
    pub fn new<T: Serialize>(val: &T) -> Result<Self> {
        let bytes = rmp_serde::to_vec_named(val)?;
        Ok(RedisStr(bytes))
    }

    /// attempt to deserialize the value
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

/// Generate a random redis key using uuid v4
#[inline(always)]
pub(crate) fn random_key(prefix: &str) -> String {
    let uuid = Uuid::new_v4();
    format!("r:{}:{}", prefix, uuid.to_string())
}

/// append user and group id to a key
#[inline(always)]
pub(crate) fn scope_key_by_user(key: &str, user: i64) -> String {
    format!("u:{}:{}", user, key)
}

#[inline(always)]
pub(crate) fn scope_key(key: &str, message: &Message, prefix: &str) -> Result<String> {
    let user_id = message
        .get_from()
        .as_ref()
        .ok_or_else(|| BotError::conversation_err("message without sender"))?
        .get_id();
    let chat_id = message.get_chat().get_id();
    let res = format!("{}:{}:{}:{}", prefix, chat_id, user_id, key);
    Ok(res)
}

#[inline(always)]
pub(crate) fn scope_key_by_chatuser(key: &str, message: &Message) -> Result<String> {
    scope_key(key, message, "cu")
}

/// Builder type for a managed pool of redis connections using bb8
pub struct RedisPoolBuilder {
    connectionstr: String,
}

/// Since redis support multiple threads in specific circumstances we use a pool for parallelism
pub struct RedisPool {
    pool: Pool<RedisConnectionManager>,
}

impl RedisPoolBuilder {
    /// Constructs a new redis pool from a connection string. Connections aren't attempted until
    /// build() is called
    pub fn new<T: ToString>(connectonstr: T) -> Self {
        RedisPoolBuilder {
            connectionstr: connectonstr.to_string(),
        }
    }

    /// Build the pool and attempt connection
    pub async fn build(self) -> Result<RedisPool> {
        RedisPool::new(self.connectionstr).await
    }
}

impl RedisPool {
    /// create a new redis pool from a connection string and immediately connect to it
    pub async fn new<T: AsRef<str>>(connectionstr: T) -> Result<Self> {
        let client = RedisConnectionManager::new(connectionstr.as_ref())?;
        //TODO: don't use a hardcoded size here
        let pool = Pool::builder().max_size(15).build(client).await?;
        Ok(RedisPool { pool })
    }

    /// atomically create a list out of multipole Serialize types
    /// any previous list at this key will be overwritten
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
                Ok::<(), BotError>(())
            })?;
            Ok(p)
        })
        .await?;
        Ok(())
    }

    /// remove and deserialize all items in a list.
    pub async fn drain_list<R>(&self, key: &str) -> Result<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync,
    {
        let mut conn = self.pool.get().await?;
        conn.lrange::<&str, Vec<Vec<u8>>>(key, 0, -1)
            .await?
            .into_iter()
            .map(|v| rmp_serde::from_slice(&v.as_slice()).map_err(|e| e.into()))
            .collect()
    }

    /// construct and run a redis pipeline using the provided closure
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

    /// construct and run a redis pipeline using the provided closure
    /// any Err type returned will abort without running the query
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

    /// Run a single redis query
    pub async fn sq<'a, T, R>(&'a self, func: T) -> Result<R>
    where
        T: for<'b> FnOnce(
                &'b mut PooledConnection<'a, RedisConnectionManager>,
            ) -> RedisFuture<'b, R>
            + Send,
        R: FromRedisValue + Send + 'a,
    {
        Ok(func(&mut self.pool.get().await?).await?)
    }

    /// Run one or more redis queries using the connection provided to the
    /// closure
    pub async fn query<'a, T, R, Fut>(&'a self, func: T) -> Result<R>
    where
        T: FnOnce(PooledConnection<'a, RedisConnectionManager>) -> Fut + Send,
        Fut: Future<Output = Result<R>> + Send,
        R: Send,
    {
        Ok(func(self.pool.get().await?).await?)
    }

    /// Run one or more redis queries using the connection provided to the
    /// closure. The closure is run via a separate tokio task
    pub async fn query_spawn<T, R, Fut>(&self, func: T) -> JoinHandle<Result<R>>
    where
        T: for<'b> FnOnce(PooledConnection<'b, RedisConnectionManager>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<R>> + Send,
        R: Send + 'static,
    {
        let r = self.clone();
        tokio::spawn(async move {
            let res = func(r.pool.get().await?).await?;
            let res: Result<R> = Ok(res);
            res
        })
    }

    /// Gets a single connection from the connection pool.
    /// NOTE: this connection will not be returned to the pool until it is dropped
    pub async fn conn<'a>(&'a self) -> Result<PooledConnection<'a, RedisConnectionManager>> {
        let res = self.pool.get().await?;
        Ok(res)
    }
}

/// Helper trait intended to be used as an extension trait for caching ORM
/// types
#[async_trait]
pub trait RedisCache<V>
where
    Self: Sized + Send,
{
    /// Cache the type in redis for the specific duration
    async fn cache_duration<K: AsRef<str> + Send>(self, key: K, expire: Duration) -> Result<V>;

    /// Cache the type in redis for the default duration specified by config
    async fn cache<K: AsRef<str> + Send>(self, key: K) -> Result<V>;

    /// Cache the type along with related types, used for caching rows from related tables
    async fn join_duration<K, J, A>(
        self,
        key: K,
        join: Vec<J>,
        expire: Duration,
    ) -> Result<(V, Vec<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send;

    /// Cache type type along with a single related types, used for caching rows from related tables
    async fn join<K, J, A>(self, key: K, join: Vec<J>) -> Result<(V, Vec<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send;

    /// Cache type type along with a single related types, used for caching rows from related tables
    async fn join_single_duration<K, J, A>(
        self,
        key: K,
        join: Option<J>,
        expire: Duration,
    ) -> Result<(V, Option<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send;

    /// Cache type type along with a single related types, used for caching rows from related tables
    /// using default expire
    async fn join_single<K, J, A>(self, key: K, join: Option<J>) -> Result<(V, Option<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send;
}

#[async_trait]
impl<T, V> RedisCache<V> for T
where
    T: Sized + Serialize + IntoActiveModel<V> + Send,
    V: ActiveModelTrait + Send,
    Self: Sized + Send,
{
    async fn cache_duration<K: AsRef<str> + Send>(self, key: K, expire: Duration) -> Result<V> {
        let st = RedisStr::new(&self)?;
        let r = key.as_ref();
        REDIS
            .pipe(|q| q.set(r, st).expire(r, expire.num_seconds() as usize))
            .await?;
        Ok(self.into_active_model())
    }

    async fn cache<K: AsRef<str> + Send>(self, key: K) -> Result<V> {
        self.cache_duration(key, Duration::seconds(CONFIG.timing.cache_timeout as i64))
            .await
    }

    async fn join_duration<K, J, A>(
        self,
        key: K,
        join: Vec<J>,
        expire: Duration,
    ) -> Result<(V, Vec<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send,
    {
        let v = (self, join);
        let st = RedisStr::new(&v)?;
        let r = key.as_ref();

        REDIS
            .pipe(|q| q.set(r, st).expire(r, expire.num_seconds() as usize))
            .await?;
        let o =
            v.1.into_iter()
                .map(|v| v.into_active_model())
                .collect::<Vec<A>>();
        Ok((v.0.into_active_model(), o))
    }

    async fn join<K, J, A>(self, key: K, join: Vec<J>) -> Result<(V, Vec<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send,
    {
        self.join_duration(
            key,
            join,
            Duration::seconds(CONFIG.timing.cache_timeout as i64),
        )
        .await
    }

    async fn join_single_duration<K, J, A>(
        self,
        key: K,
        join: Option<J>,
        expire: Duration,
    ) -> Result<(V, Option<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send,
    {
        let v = (self, join);
        let st = RedisStr::new(&v)?;
        let r = key.as_ref();

        REDIS
            .pipe(|q| q.set(r, st).expire(r, expire.num_seconds() as usize))
            .await?;
        let o = v.1.map(|v| v.into_active_model());
        Ok((v.0.into_active_model(), o))
    }

    async fn join_single<K, J, A>(self, key: K, join: Option<J>) -> Result<(V, Option<A>)>
    where
        J: Sized + Serialize + IntoActiveModel<A> + Send,
        A: ActiveModelTrait + Send,
        K: AsRef<str> + Send,
    {
        self.join_single_duration(
            key,
            join,
            Duration::seconds(CONFIG.timing.cache_timeout as i64),
        )
        .await
    }
}

impl Clone for RedisPool {
    fn clone(&self) -> Self {
        RedisPool {
            pool: self.pool.clone(),
        }
    }
}
