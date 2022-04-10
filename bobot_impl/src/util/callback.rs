use futures::{future::BoxFuture, Future, FutureExt};
use serde::de::DeserializeOwned;

use crate::persist::Result;

pub type BotDbFuture<'a, T> = BoxFuture<'a, T>;

// async closure type returning boxed future
pub trait DbCallback<T, U>: for<'a> FnOnce(&'a T) -> BotDbFuture<'a, U> + Send + Sync {}
impl<F, T, U> DbCallback<T, U> for F where
    F: for<'a> FnOnce(&'a T) -> BotDbFuture<'a, U> + Send + Sync
{
}
// async closure type returning unboxed future.
// This type can be used as a plain async FnOnce, or as an async
// version of std::boxed::FmBox
//
// Note: To be able to call an FnOnce type, we need to consume self.
pub trait SingleCallback<'a, T, U>: Send + Sync {
    type Fut: 'a + Future<Output = U> + Send;
    fn cb(self, val: &'a T) -> Self::Fut;
}

pub trait BiCallback<'a, T, U, R>: Send + Sync {
    type Fut: 'a + Future<Output = R> + Send;
    fn cb(self, val: &'a T, val2: &'a U) -> Self::Fut;
}

pub trait BoxedSingleCallback<'a, T, U>: Send + Sync {
    type Fut: 'a + Future<Output = U> + Send;
    fn cb_boxed(self: Box<Self>, val: &'a T) -> Self::Fut;
}

pub trait BoxedBiCallback<'a, T, U, R>: Send + Sync {
    type Fut: 'a + Future<Output = R> + Send;
    fn cb_boxed_bi(self: Box<Self>, val: &'a T, val2: &'a U) -> Self::Fut;
}

// all functions that meet the criteria implement the trait
// (currently FnOnce returning future)
impl<'a, F, T, Fut, U> SingleCallback<'a, T, U> for F
where
    T: 'a,
    F: FnOnce(&'a T) -> Fut + Send + Sync,
    Fut: 'a + Future<Output = U> + Send,
{
    type Fut = Fut;
    fn cb(self, val: &'a T) -> Self::Fut {
        self(val)
    }
}

impl<'a, F, T, Fut, U> BoxedSingleCallback<'a, T, U> for F
where
    T: 'a,
    F: FnOnce(&'a T) -> Fut + Send + Sync,
    Fut: 'a + Future<Output = U> + Send,
{
    type Fut = Fut;
    fn cb_boxed(self: Box<Self>, val: &'a T) -> Self::Fut {
        (*self)(val)
    }
}

impl<'a, F, T, Fut, U, R> BiCallback<'a, T, U, R> for F
where
    T: 'a,
    U: 'a,
    F: FnOnce(&'a T, &'a U) -> Fut + Send + Sync,
    Fut: 'a + Future<Output = R> + Send,
{
    type Fut = Fut;
    fn cb(self, val: &'a T, val2: &'a U) -> Self::Fut {
        self(val, val2)
    }
}

impl<'a, F, T, Fut, U, R> BoxedBiCallback<'a, T, U, R> for F
where
    T: 'a,
    U: 'a,
    F: FnOnce(&'a T, &'a U) -> Fut + Send + Sync,
    Fut: 'a + Future<Output = R> + Send,
{
    type Fut = Fut;
    fn cb_boxed_bi(self: Box<Self>, val: &'a T, val2: &'a U) -> Self::Fut {
        (*self)(val, val2)
    }
}

// type erasure on the future
pub(crate) struct OutputBoxer<F>(pub(crate) F);
impl<'a, F, T, U> BoxedSingleCallback<'a, T, U> for OutputBoxer<F>
where
    F: SingleCallback<'a, T, U>,
    U: 'a,
{
    type Fut = BotDbFuture<'a, U>;
    fn cb_boxed(self: Box<Self>, val: &'a T) -> Self::Fut {
        (*self).0.cb(val).boxed()
    }
}

impl<'a, F, T, U, R> BoxedBiCallback<'a, T, U, R> for OutputBoxer<F>
where
    F: BiCallback<'a, T, U, R>,
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;
    fn cb_boxed_bi(self: Box<Self>, val: &'a T, val2: &'a U) -> Self::Fut {
        (*self).0.cb(val, val2).boxed()
    }
}

// type erFutction, with the future already type-erased
pub struct BoxedDbCallback<T, U>(
    Box<dyn for<'a> BoxedSingleCallback<'a, T, U, Fut = BotDbFuture<'a, U>>>,
);

pub struct BoxedBiCallbackStruct<T, U, R>(
    Box<dyn for<'a> BoxedBiCallback<'a, T, U, R, Fut = BotDbFuture<'a, R>>>,
);

impl<'a, T, U: 'a> BoxedDbCallback<T, U> {
    pub fn new<F>(f: F) -> Self
    where
        F: for<'b> SingleCallback<'b, T, U> + 'static,
        U: 'static,
    {
        Self(Box::new(OutputBoxer(f)))
    }
}

impl<'a, T, U, R: 'a> BoxedBiCallbackStruct<T, U, R> {
    pub fn new<F>(f: F) -> Self
    where
        F: for<'b> BiCallback<'b, T, U, R> + 'static,
        R: 'static,
    {
        Self(Box::new(OutputBoxer(f)))
    }
}

impl<'a, T, U> SingleCallback<'a, T, U> for BoxedDbCallback<T, U>
where
    U: 'a,
{
    type Fut = BotDbFuture<'a, U>;

    fn cb(self, val: &'a T) -> Self::Fut {
        self.0.cb_boxed(val)
    }
}

impl<'a, T, U, R> BiCallback<'a, T, U, R> for BoxedBiCallbackStruct<T, U, R>
where
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;

    fn cb(self, val: &'a T, val2: &'a U) -> Self::Fut {
        self.0.cb_boxed_bi(val, val2)
    }
}

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
