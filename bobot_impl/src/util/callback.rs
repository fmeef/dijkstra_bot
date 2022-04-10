use futures::{future::BoxFuture, Future, FutureExt};
use serde::de::DeserializeOwned;

use crate::persist::Result;

pub type BotDbFuture<'a, T> = BoxFuture<'a, T>;

// type erasure on the future
pub(crate) struct OutputBoxer<F>(pub(crate) F);

pub(crate) struct CacheCb<T, R>(
    pub(crate) Box<dyn for<'a> BoxedCacheCallback<'a, T, R, Fut = BotDbFuture<'a, Result<Option<R>>>>>,
);

impl<'a, T, R> CacheCallback<'a, T, R> for CacheCb<T, R>
where
    R: DeserializeOwned + 'static,
{
    type Fut = BotDbFuture<'a, Result<Option<R>>>;
    fn cb(self, key: String, db: T) -> Self::Fut {
        self.0.cb_boxed(key, db)
    }
}

pub trait CacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<Option<R>>> + Send + 'a;
    fn cb(self, key: String, db: T) -> Self::Fut;
}

pub trait BoxedCacheCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = Result<Option<R>>> + Send + 'a;
    fn cb_boxed(self: Box<Self>, key: String, db: T) -> Self::Fut;
}

impl<'a, F, T, R, Fut> CacheCallback<'a, T, R> for F
where
    F: FnOnce(String, T) -> Fut + Sync + Send + 'static,
    R: DeserializeOwned + 'static,
    Fut: Future<Output = Result<Option<R>>> + Send + 'static,
{
    type Fut = Fut;
    fn cb(self, key: String, db: T) -> Self::Fut {
        self(key, db)
    }
}

impl<'a, F, T, R> BoxedCacheCallback<'a, T, R> for OutputBoxer<F>
where
    F: CacheCallback<'a, T, R>,
    R: 'a,
{
    type Fut = BotDbFuture<'a, Result<Option<R>>>;
    fn cb_boxed(self: Box<Self>, key: String, db: T) -> Self::Fut {
        (*self).0.cb(key, db).boxed()
    }
}

impl<'a, F, T, R, Fut> BoxedCacheCallback<'a, T, R> for F
where
    F: FnOnce(String, T) -> Fut + Sync + Send + 'static,
    R: DeserializeOwned + 'static,
    Fut: Future<Output = Result<Option<R>>> + Send + 'static,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, key: String, db: T) -> Self::Fut {
        (*self)(key, db)
    }
}
