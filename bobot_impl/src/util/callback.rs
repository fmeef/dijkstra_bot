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
pub(crate) struct OutputBoxer<F>(pub(crate) F);


// boxed closure type returning future for handling cached redis/sql queries
pub(crate) struct CacheCb<T, R>(
    pub(crate) Box<dyn for<'a> BoxedCacheCallback<'a, T, R, Fut = BotDbFuture<'a, Result<Option<R>>>>>,
);

impl <'a, T, R: 'a> CacheCb<T, R> {

    pub(crate) fn new<F>(func: F) -> Self 
    where 
        F: for<'b> CacheCallback<'b, T, R> + 'static,
    R: DeserializeOwned + 'static
    {
        Self(Box::new(OutputBoxer(func)))
        
    }
}

impl<'a, T, R> CacheCallback<'a, T, R> for CacheCb<T, R>
where
    R: DeserializeOwned + 'a,
{
    type Fut = BotDbFuture<'a, Result<Option<R>>>;
    fn cb(self, key: &'a String, db: &'a T) -> Self::Fut {
        self.0.cb_boxed(key, db)
    }
}

pub trait CacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<Option<R>>> + Send + 'a;
    fn cb(self, key: &'a String, db: &'a T) -> Self::Fut;
}

pub trait BoxedCacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<Option<R>>> + Send + 'a;
    fn cb_boxed(self: Box<Self>, key: &'a String, db: &'a T) -> Self::Fut;
}

impl<'a, F, T, R, Fut> CacheCallback<'a, T, R> for F
where
    F: FnOnce(&'a String, &'a T) -> Fut + Sync + Send + 'a,
    R: DeserializeOwned + 'a,
    T: 'a,
    Fut: Future<Output = Result<Option<R>>> + Send + 'a,
{
    type Fut = Fut;
    fn cb(self, key: &'a String, db: &'a T) -> Self::Fut {
        self(key, db)
    }
}

impl<'a, F, T, R> BoxedCacheCallback<'a, T, R> for OutputBoxer<F>
where
    F: CacheCallback<'a, T, R>,
    R: DeserializeOwned + 'a
{
    type Fut = BotDbFuture<'a, Result<Option<R>>>;
    fn cb_boxed(self: Box<Self>, key: &'a String, db: &'a T) -> Self::Fut {
        (*self).0.cb(key, db).boxed()
    }
}

impl<'a, F, T, R, Fut> BoxedCacheCallback<'a, T, R> for F
where
    F: FnOnce(&'a String, &'a T) -> Fut + Sync + Send + 'a,
    R: 'a,
    T: 'a,
    Fut: Future<Output = Result<Option<R>>> + Send + 'a,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, key: &'a String, db: &'a T) -> Self::Fut {
        (*self)(key, db)
    }
}


// Boxed closure type returning future for updating redis on cache miss
pub(crate) struct CacheMissCb<T, V>(
    pub(crate) Box<dyn for<'a> BoxedCacheMissCallback<'a, T,V,Fut = BotDbFuture<'a, Result<()>>>>,
);
impl<'a, T, V> CacheMissCallback<'a, T, V> for CacheMissCb<T, V>
where
    V: Serialize + 'static,
{
    type Fut = BotDbFuture<'a, Result<()>>;
    fn cb(self, key: &String, val: &V, db: &T) -> Self::Fut {
        self.0.cb_boxed(key, val, db)
    }
}

pub trait CacheMissCallback<'a, T, V>: Send + Sync {
    type Fut: Future<Output = Result<()>> + Send + 'a;
    fn cb(self, key: &String, val: &V, db: &T) -> Self::Fut;
}

pub trait BoxedCacheMissCallback<'a, T, V>: Send + Sync {
    type Fut: Future<Output = Result<()>> + Send + 'a;
    fn cb_boxed(self: Box<Self>, key: &String, val: &V, db: &T) -> Self::Fut;
}

impl<'a, F, T, V, Fut> CacheMissCallback<'a, T, V> for F
where
    F: for<'b> FnOnce(&'b String, &'b V, &'b T) -> Fut + Sync + Send + 'static,
    V: Serialize + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    type Fut = Fut;
    fn cb(self, key: &String, val: &V, db: &T) -> Self::Fut {
        self(key, val, db)
    }
}

impl<'a, F, T, V> BoxedCacheMissCallback<'a, T, V> for OutputBoxer<F>
where
    F: CacheMissCallback<'a, T, V>,
    V: Serialize + 'static, 
{
    type Fut = BotDbFuture<'a, Result<()>>;
    fn cb_boxed(self: Box<Self>, key: &String, val: &V, db: &T) -> Self::Fut {
        (*self).0.cb(key,val,  db).boxed()
    }
}

impl<'a, F, T, V, Fut> BoxedCacheMissCallback<'a, T, V> for F
where
    F: for<'b> FnOnce(&'b String, &'b V, &'b T) -> Fut + Sync + Send + 'static,
    V: Serialize + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, key: &String, val: &V, db: &T) -> Self::Fut {
        (*self)(key, val, db)
    }
}
