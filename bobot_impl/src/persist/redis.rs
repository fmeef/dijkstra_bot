use super::DbTable;
use super::Result;
use crate::util::callback::SingleCallback;
use crate::{
    get_executor,
    util::{
        callback::BoxedDbCallback,
        chat_id_fix::{ChatRefExt, UserIdExt},
        error::BotError,
    },
};
use anyhow::anyhow;
use async_nursery::{NurseExt, Nursery};
use async_trait::async_trait;
use bb8::{Pool, PooledConnection};
use bb8_redis::RedisConnectionManager;
use dashmap::DashMap;
use futures::{Future, Stream, TryStreamExt};
use redis::{aio::Connection, AsyncCommands, RedisError};
use sea_orm::DatabaseConnection;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, num::NonZeroUsize, ops::DerefMut, pin::Pin, sync::Arc};
use telegram_bot::{Message, ToUserId};
use uuid::Uuid;

// write cache redis keys
pub const KEY_WRITE_CACHE: &str = "writecache";
pub const KEY_TYPE_PREFIX: &str = "wc:typeprefix";
pub const KEY_WRAPPER: &str = "wc:wrapper";
pub const KEY_TYPE_VAL: &str = "wc:typeval";
pub const KEY_UUID: &str = "wc:uuid";

pub type ParserResult = Result<Box<dyn DbTable<DatabaseConnection> + Send + Sync>>;

pub trait Parser: Fn(&Vec<u8>) -> ParserResult + Send + Sync + 'static {}

pub type BoxedParser = Box<dyn Parser>;

pub type BacklogStream = Pin<Box<dyn Stream<Item = Result<WriteCacheItem>> + Send + Sync>>;

pub fn error_mapper(err: RedisError) -> BotError {
    match err.kind() {
        _ => BotError::new("some redis error"),
    }
}

pub fn make_parser<F>(func: F) -> BoxedParser
where
    F: Parser,
{
    Box::new(func)
}

pub fn random_key<T: AsRef<str>>(prefix: &T) -> String {
    let uuid = Uuid::new_v4();
    format!("r:{}:{}", prefix.as_ref(), uuid.to_string())
}

pub fn scope_key_by_user<T: AsRef<str>, U: ToUserId>(key: &T, user: &U) -> String {
    format!("u:{}:{}", user.to_user_i64(), key.as_ref())
}

pub fn scope_key_by_chatuser<T: AsRef<str>>(key: &T, message: &Message) -> String {
    let user_id = message.from.to_user_i64();
    let chat_id = message.chat.to_chat_id();
    format!("cu:{}:{}:{}", chat_id, user_id, key.as_ref())
}

fn parse_single_type_prefix<'a, T>(
    data: &'a Vec<u8>,
) -> Result<Box<dyn DbTable<DatabaseConnection> + Send + Sync>>
where
    T: DbTable<DatabaseConnection> + Deserialize<'a> + Send + Sync + 'static,
{
    let t: T = rmp_serde::from_read_ref(data.as_slice())?;
    Ok(Box::new(t))
}

fn parse_type_prefix_rb<'a>(
    m: Arc<HashMap<String, BoxedParser>>,
    t: &str,
    data: &'a Vec<u8>,
) -> Result<Box<dyn DbTable<DatabaseConnection> + Send + Sync>> {
    if let Some(parser) = m.get(t) {
        parser(data)
    } else {
        Err(anyhow!(BotError::new("nonexistent parser")))
    }
}

pub trait RedisTypeName {
    fn get_type_name(&self) -> &'static str;
}

impl<F> Parser for F where F: Fn(&Vec<u8>) -> ParserResult + Send + Sync + 'static {}

#[async_trait]
trait ConnectionExt {
    async fn create_obj<T, V>(&mut self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static;

    async fn create_obj_expire<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static;

    async fn create_obj_expire_at<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static;

    async fn get_type_name<T>(&mut self, key: &T) -> Result<String>
    where
        T: AsRef<str> + Send + Sync + 'static;
    async fn get_type_data<T>(&mut self, key: &T) -> Result<Vec<u8>>
    where
        T: AsRef<str> + Send + Sync + 'static;
}

#[async_trait]
pub trait WriteCache<T, U>
where
    T: 'static,
{
    async fn get_backlog(&self, size: NonZeroUsize) -> Result<BacklogStream>;
    async fn estimate_backlog(&self) -> Result<usize>;
}

pub struct RedisPoolBuilder {
    connectionstr: String,
    parsers: HashMap<String, BoxedParser>,
}

pub struct RedisPool {
    pool: Pool<RedisConnectionManager>,
    parsers: Arc<HashMap<String, BoxedParser>>,
    writecallbacks: Arc<DashMap<String, BoxedDbCallback<Result<Uuid>, Result<()>>>>,
}

pub struct WriteCacheItem {
    pub id: Uuid,
    callback: BoxedDbCallback<Result<Uuid>, Result<()>>,
    payload: Vec<u8>,
    typeprefix: String,
    parsers: Arc<HashMap<String, BoxedParser>>,
    wrapper: Option<String>,
}

impl WriteCacheItem {
    pub fn to_db(&self) -> Result<Box<dyn DbTable<DatabaseConnection> + Send + Sync>> {
        parse_type_prefix_rb(self.parsers.clone(), &self.typeprefix, &self.payload)
    }

    pub async fn insert_with_wrapper(self, pool: &DatabaseConnection) -> Result<()> {
        let dbtable = self.to_db()?;
        if let Err(err) = dbtable.insert(pool, self.wrapper).await {
            self.callback.cb(&Err(err)).await
        } else {
            self.callback.cb(&Ok(self.id)).await
        }
    }
}

#[async_trait]
impl DbTable<DatabaseConnection> for WriteCacheItem {
    async fn insert(&self, pool: &DatabaseConnection, wrapper: Option<String>) -> Result<()> {
        let dbtable = self.to_db()?;
        dbtable.insert(pool, wrapper).await
    }
}

#[async_trait]
impl ConnectionExt for Connection {
    async fn create_obj<T, V>(&mut self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let type_name = obj.get_type_name().to_string();
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_PREFIX, type_name)
            .hset(key.as_ref(), KEY_TYPE_VAL, obj)
            .query_async(self)
            .await?;
        Ok(())
    }

    async fn create_obj_expire<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let type_name = obj.get_type_name().to_string();
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_PREFIX, type_name)
            .hset(key.as_ref(), KEY_TYPE_VAL, obj)
            .expire(key.as_ref(), seconds)
            .query_async(self)
            .await?;
        Ok(())
    }

    async fn create_obj_expire_at<T, V>(&mut self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let type_name = obj.get_type_name().to_string();
        let obj = rmp_serde::to_vec(&obj)?;
        redis::pipe()
            .atomic()
            .hset(key.as_ref(), KEY_TYPE_PREFIX, type_name)
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
            parsers: HashMap::new(),
        }
    }

    pub fn add_parser<T: Parser, U: ToString>(mut self, key: U, parser: T) -> Self {
        let boxed = Box::new(parser);
        self.parsers.insert(key.to_string(), boxed);
        self
    }

    pub fn add_parsers(mut self, parsers: HashMap<String, BoxedParser>) -> Self {
        for (key, value) in parsers.into_iter() {
            self.parsers.insert(key, value);
        }

        self
    }

    pub async fn build(self) -> Result<RedisPool> {
        RedisPool::new(self.connectionstr, self.parsers).await
    }
}

impl RedisPool {
    pub async fn new<T: AsRef<str>>(
        connectionstr: T,
        parsers: HashMap<String, BoxedParser>,
    ) -> Result<Self> {
        let client = RedisConnectionManager::new(connectionstr.as_ref())?;

        let pool = Pool::builder().max_size(15).build(client).await?;
        Ok(RedisPool {
            pool,
            parsers: Arc::new(parsers),
            writecallbacks: Arc::new(DashMap::new()),
        })
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
        V: DeserializeOwned + RedisTypeName + Sync + Send + 'static,
    {
        let mut conn = self.pool.get().await?;
        let d = conn.get_type_data(key).await?;
        let obj: V = rmp_serde::from_read_ref(d.as_slice())?;
        Ok(obj)
    }

    pub async fn create_obj<T, V>(&self, key: &T, obj: &V) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        conn.create_obj(key, obj).await
    }

    pub async fn create_obj_auto<V>(&self, obj: &V) -> Result<String>
    where
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj(&key, obj).await?;
        Ok(key)
    }

    pub async fn create_obj_expire<T, V>(&self, key: &T, obj: &V, seconds: usize) -> Result<()>
    where
        T: AsRef<str> + Send + Sync + 'static,
        V: Serialize + RedisTypeName + Send + Sync + 'static,
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
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        conn.create_obj_expire_at(key, obj, seconds).await?;
        Ok(())
    }

    pub async fn create_obj_expire_auto<V>(&self, obj: &V, seconds: usize) -> Result<String>
    where
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj_expire(&key, obj, seconds).await?;
        Ok(key)
    }

    pub async fn create_obj_expire_at_auto<V>(&self, obj: &V, seconds: usize) -> Result<String>
    where
        V: Serialize + RedisTypeName + Send + Sync + 'static,
    {
        let mut conn = self.pool.get().await?;
        let key = random_key(&"");
        conn.create_obj_expire_at(&key, obj, seconds).await?;
        Ok(key)
    }

    pub async fn persist_list<T: AsRef<str>>(&self, key: &T) -> Result<()> {
        self.pop_writecache_all(key.as_ref())
            .await?
            .try_for_each_concurrent(None, |_v| async move {
                //v.insert(&DB.get_engine(), None).await?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn persist_key<T: AsRef<str>>(&self, key: &T) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let t: String = redis::cmd("TYPE")
            .arg(key.as_ref())
            .query_async(conn.deref_mut())
            .await?;

        drop(conn);
        match t.as_str() {
            "list" => self.persist_list(key).await,
            _ => Err(anyhow!(BotError::new("invalid redis type"))),
        }
    }

    pub async fn push_writecache<T>(&self, v: T) -> Result<Uuid>
    where
        T: Serialize + RedisTypeName + Send + 'static,
    {
        self.push_writecache_cb(v, |_: &Result<Uuid>| async move { Ok(()) })
            .await
    }

    async fn push_writecache_cb<T, F>(&self, v: T, callback: F) -> Result<Uuid>
    where
        T: Serialize + RedisTypeName + Send + 'static,
        F: for<'a> SingleCallback<'a, Result<Uuid>, Result<()>> + 'static,
    {
        self.push_writecache_internal(v, callback, None).await
    }

    async fn push_writecache_wrapper<T, F>(
        &self,
        v: T,
        callback: F,
        wrapper: String,
    ) -> Result<Uuid>
    where
        T: Serialize + RedisTypeName + Send + 'static,
        F: for<'a> SingleCallback<'a, Result<Uuid>, Result<()>> + 'static,
    {
        self.push_writecache_internal(v, callback, Some(wrapper))
            .await
    }

    async fn push_writecache_internal<T, F>(
        &self,
        v: T,
        callback: F,
        wrapper: Option<String>,
    ) -> Result<Uuid>
    where
        T: Serialize + RedisTypeName + Send + 'static,
        F: for<'a> SingleCallback<'a, Result<Uuid>, Result<()>> + 'static,
    {
        let mut conn = self.pool.get().await?;

        let type_name = v.get_type_name().to_string();

        let obj = rmp_serde::to_vec(&v)?;

        let wrapper = rmp_serde::to_vec(&wrapper)?;
        let uuid = Uuid::new_v4();
        let uuidbytes: Vec<u8> = rmp_serde::to_vec(&uuid)?;
        let key = random_key(&"");
        let res: std::result::Result<(), RedisError> = redis::pipe()
            .atomic()
            .hset(&key, KEY_TYPE_PREFIX, type_name)
            .hset(&key, KEY_TYPE_VAL, obj)
            .hset(&key, KEY_WRAPPER, wrapper)
            .hset(&key, KEY_UUID, uuidbytes)
            .lpush(KEY_WRITE_CACHE, &key)
            .query_async(conn.deref_mut())
            .await;
        if let Err(err) = res {
            let res: Result<Uuid> = Err(anyhow!(BotError::RedisErr(err)));
            callback.cb(&res).await?;
            res
        } else {
            self.writecallbacks
                .insert(key, BoxedDbCallback::new(callback));
            Ok(uuid)
        }
    }
    pub fn pop_cb<T: AsRef<str>>(&self, key: &T) -> BoxedDbCallback<Result<Uuid>, Result<()>> {
        if let Some((_, cb)) = self.writecallbacks.remove(key.as_ref()) {
            cb
        } else {
            BoxedDbCallback::new(|_: &Result<Uuid>| async move { Ok(()) })
        }
    }
    pub async fn pop_writecache_all(&self, listkey: &str) -> Result<BacklogStream> {
        let mut conn = self.pool.get().await?;

        let size: usize = conn.llen(listkey).await?;
        drop(conn);
        self.pop_writecache(
            listkey,
            NonZeroUsize::new(size).ok_or_else(move || BotError::new("zero size"))?,
        )
        .await
    }

    pub async fn pop_writecache(
        &self,
        listkey: &str,
        count: NonZeroUsize,
    ) -> Result<BacklogStream> {
        let mut conn = self.pool.get().await?;

        let keys_many: Vec<String> = conn.rpop(listkey, Some(count)).await?;

        let keys: Vec<String> = if keys_many.len() == 0 {
            let (_, v): (String, String) = conn.brpop(listkey, 0).await?;
            vec![v]
        } else {
            keys_many
        };

        let (nursury, output) = Nursery::new(get_executor());
        for key in keys {
            let pool = self.pool.clone();
            let parsers = Arc::clone(&self.parsers);
            let self_ref = self.clone();
            nursury.nurse(async move {
                let mut conn = pool.get().await?;

                let t: String = conn.hget(&key, KEY_TYPE_PREFIX).await?;

                let data: Vec<u8> = conn.hget(&key, KEY_TYPE_VAL).await?;
                let wrapper: Vec<u8> = conn.hget(&key, KEY_WRAPPER).await?;
                let wrapper: Option<String> = rmp_serde::from_read_ref(wrapper.as_slice())?;

                let id: Vec<u8> = conn.hget(&key, KEY_UUID).await?;
                let id: Uuid = rmp_serde::from_read_ref(id.as_slice())?;
                let t = WriteCacheItem {
                    callback: self_ref.pop_cb(&key),
                    payload: data,
                    typeprefix: t,
                    parsers,
                    wrapper,
                    id,
                };
                Ok(t)
            })?;
        }

        drop(nursury);
        Ok(Box::pin(output))
    }
}

impl Clone for RedisPool {
    fn clone(&self) -> Self {
        RedisPool {
            pool: self.pool.clone(),
            parsers: Arc::clone(&self.parsers),
            writecallbacks: Arc::clone(&self.writecallbacks),
        }
    }
}

#[async_trait]
impl WriteCache<Pool<RedisConnectionManager>, DatabaseConnection> for RedisPool {
    async fn get_backlog(&self, size: NonZeroUsize) -> Result<BacklogStream> {
        self.pop_writecache(KEY_WRITE_CACHE, size).await
    }

    async fn estimate_backlog(&self) -> Result<usize> {
        self.get_cache_size().await
    }
}
