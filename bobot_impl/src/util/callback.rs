/**
 * NOTE: async closures are not stable, so defining closure types returning future
 * (and supporting both type erasure and sized-ness for storing in collections)
 * are extremely ugly and full of annoying boilerplate.
 *
 * This is the containment module for async-closure related workarounds until we get stable
 * support for native async closures
 */
use futures::{future::BoxFuture, Future, FutureExt};
use serde::{de::DeserializeOwned, Serialize};

use crate::persist::Result;

pub type BotDbFuture<'a, T> = BoxFuture<'a, T>;

// type erasure on the future
pub struct OutputBoxer<F>(pub F);

// boxed closure type returning future for handling cached redis/sql queries
pub struct CacheCb<T, R>(
    Box<dyn for<'a> BoxedCacheCallback<'a, T, R, Fut = BotDbFuture<'a, Result<R>>>>,
);

pub struct SingleCb<T, R>(Box<dyn for<'a> BoxedSingleCallback<'a, T, R, Fut = BotDbFuture<'a, R>>>);

impl<'a, T, R: 'a> CacheCb<T, R> {
    pub fn new<F>(func: F) -> Self
    where
        F: for<'b> CacheCallback<'b, T, R> + 'static,
        R: DeserializeOwned + 'static,
    {
        Self(Box::new(OutputBoxer(func)))
    }
}

impl<'a, T, R: 'a> SingleCb<T, R> {
    pub fn new<F>(func: F) -> Self
    where
        F: for<'b> SingleCallback<'b, T, R> + 'static,
        R: 'static,
    {
        Self(Box::new(OutputBoxer(func)))
    }
}

impl<'a, T, R> CacheCallback<'a, T, R> for CacheCb<T, R>
where
    R: DeserializeOwned + 'a,
{
    type Fut = BotDbFuture<'a, Result<R>>;
    fn cb(self, key: &'a str, db: &'a T) -> Self::Fut {
        self.0.cb_boxed(key, db)
    }
}

impl<'a, T, R> SingleCallback<'a, T, R> for SingleCb<T, R>
where
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;
    fn cb(self, db: T) -> Self::Fut {
        self.0.cb_boxed(db)
    }
}

pub trait SingleCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb(self, db: T) -> Self::Fut;
}
pub trait CacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<R>> + Send + 'a;
    fn cb(self, key: &'a str, db: &'a T) -> Self::Fut;
}

pub trait BoxedSingleCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb_boxed(self: Box<Self>, db: T) -> Self::Fut;
}

pub trait BoxedCacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<R>> + Send + 'a;
    fn cb_boxed(self: Box<Self>, key: &'a str, db: &'a T) -> Self::Fut;
}

impl<'a, F, T, R, Fut> CacheCallback<'a, T, R> for F
where
    F: FnOnce(&'a str, &'a T) -> Fut + Sync + Send + 'a,
    R: DeserializeOwned,
    T: 'a,
    Fut: Future<Output = Result<R>> + Send + 'a,
{
    type Fut = Fut;
    fn cb(self, key: &'a str, db: &'a T) -> Self::Fut {
        self(key, db)
    }
}

impl<'a, F, T, R, Fut> SingleCallback<'a, T, R> for F
where
    F: FnOnce(T) -> Fut + Sync + Send + 'a,
    T: 'a,
    R: 'a,
    Fut: Future<Output = R> + Send + 'a,
{
    type Fut = Fut;
    fn cb(self, db: T) -> Self::Fut {
        self(db)
    }
}

impl<'a, F, T, R> BoxedSingleCallback<'a, T, R> for OutputBoxer<F>
where
    F: SingleCallback<'a, T, R>,
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;
    fn cb_boxed(self: Box<Self>, db: T) -> Self::Fut {
        (*self).0.cb(db).boxed()
    }
}

impl<'a, F, T, R> BoxedCacheCallback<'a, T, R> for OutputBoxer<F>
where
    F: CacheCallback<'a, T, R>,
    R: DeserializeOwned + 'a,
{
    type Fut = BotDbFuture<'a, Result<R>>;
    fn cb_boxed(self: Box<Self>, key: &'a str, db: &'a T) -> Self::Fut {
        (*self).0.cb(key, db).boxed()
    }
}

impl<'a, F, T, R, Fut> BoxedCacheCallback<'a, T, R> for F
where
    F: FnOnce(&'a str, &'a T) -> Fut + Sync + Send + 'a,
    R: 'a,
    T: 'a,
    Fut: Future<Output = Result<R>> + Send + 'a,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, key: &'a str, db: &'a T) -> Self::Fut {
        (*self)(key, db)
    }
}

// Boxed closure type returning future for updating redis on cache miss
pub struct CacheMissCb<T, V>(
    pub Box<dyn for<'a> BoxedCacheMissCallback<'a, T, V, Fut = BotDbFuture<'a, Result<V>>>>,
);
impl<'a, T, V> CacheMissCallback<'a, T, V> for CacheMissCb<T, V>
where
    V: Serialize + 'a,
{
    type Fut = BotDbFuture<'a, Result<V>>;
    fn cb(self, key: &'a str, val: V, db: &'a T) -> Self::Fut {
        self.0.cb_boxed(key, val, db)
    }
}

pub trait CacheMissCallback<'a, T, V>: Send + Sync {
    type Fut: Future<Output = Result<V>> + Send + 'a;
    fn cb(self, key: &'a str, val: V, db: &'a T) -> Self::Fut;
}

pub trait BoxedCacheMissCallback<'a, T, V>: Send + Sync {
    type Fut: Future<Output = Result<V>> + Send + 'a;
    fn cb_boxed(self: Box<Self>, key: &'a str, val: V, db: &'a T) -> Self::Fut;
}

impl<'a, F, T, V, Fut> CacheMissCallback<'a, T, V> for F
where
    F: FnOnce(&'a str, V, &'a T) -> Fut + Sync + Send + 'a,
    V: Serialize + 'a,
    T: 'a,
    Fut: Future<Output = Result<V>> + Send + 'a,
{
    type Fut = Fut;
    fn cb(self, key: &'a str, val: V, db: &'a T) -> Self::Fut {
        self(key, val, db)
    }
}

impl<'a, F, T, V> BoxedCacheMissCallback<'a, T, V> for OutputBoxer<F>
where
    F: CacheMissCallback<'a, T, V>,
    V: Serialize + 'a,
{
    type Fut = BotDbFuture<'a, Result<V>>;
    fn cb_boxed(self: Box<Self>, key: &'a str, val: V, db: &'a T) -> Self::Fut {
        (*self).0.cb(key, val, db).boxed()
    }
}

impl<'a, F, T, V, Fut> BoxedCacheMissCallback<'a, T, V> for F
where
    F: FnOnce(&'a str, V, &'a T) -> Fut + Sync + Send + 'a,
    V: Serialize + 'a,
    T: 'a,
    Fut: Future<Output = Result<V>> + Send + 'a,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, key: &'a str, val: V, db: &'a T) -> Self::Fut {
        (*self)(key, val, db)
    }
}
