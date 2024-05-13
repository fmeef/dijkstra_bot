//! NOTE: async closures are not stable, so defining closure types returning future
//! (and supporting both type erasure and sized-ness for storing in collections)
//! are extremely ugly and full of annoying boilerplate.
//!
//! This is the containment module for async-closure related workarounds until we get stable
//! support for native async closures

use crate::util::error::Result;
use futures::{future::BoxFuture, Future, FutureExt};
use serde::{de::DeserializeOwned, Serialize};
pub type BotDbFuture<'a, T> = BoxFuture<'a, T>;

// type erasure on the future
pub struct OutputBoxer<F>(pub F);

pub struct SingleCb<T, R>(Box<dyn for<'a> BoxedSingleCallback<'a, T, R, Fut = BotDbFuture<'a, R>>>);

impl<T, R> std::fmt::Debug for SingleCb<T, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("its a function uwu")?;
        Ok(())
    }
}

pub struct MultiCb<T, R>(Box<dyn for<'a> BoxedMultiCallback<'a, T, R, Fut = BotDbFuture<'a, R>>>);
impl<'a, T, R: 'a> SingleCb<T, R> {
    pub fn new<F>(func: F) -> Self
    where
        F: for<'b> SingleCallback<'b, T, R> + 'static,
        R: 'static,
    {
        Self(Box::new(OutputBoxer(func)))
    }
}

impl<T, R> std::fmt::Debug for MultiCb<T, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("its a two arg function uwu")?;
        Ok(())
    }
}

impl<'a, T, R: 'a> MultiCb<T, R> {
    pub fn new<F>(func: F) -> Self
    where
        F: for<'b> MultiCallback<'b, T, R> + 'static,
        R: 'static,
    {
        Self(Box::new(OutputBoxer(func)))
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

impl<'a, T, R> MultiCallback<'a, T, R> for MultiCb<T, R>
where
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;
    fn cb(&self, db: T) -> Self::Fut {
        self.0.cb_boxed(db)
    }
}

pub trait MultiCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb(&self, db: T) -> Self::Fut;
}

pub trait SingleCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb(self, db: T) -> Self::Fut;
}
pub trait CacheCallback<'a, R, P>: Send + Sync {
    type Fut: Future<Output = Result<R>> + Send + 'a;
    fn cb(self, key: &'a str, param: &'a P) -> Self::Fut;
}

pub trait BoxedSingleCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb_boxed(self: Box<Self>, db: T) -> Self::Fut;
}

pub trait BoxedMultiCallback<'a, T, R>: Send + Sync {
    type Fut: Future<Output = R> + Send + 'a;
    fn cb_boxed(&self, db: T) -> Self::Fut;
}

impl<'a, F, R, P, Fut> CacheCallback<'a, R, P> for F
where
    F: FnOnce(&'a str, &'a P) -> Fut + Sync + Send + 'a,
    R: DeserializeOwned,
    P: Send + Sync + 'a,
    Fut: Future<Output = Result<R>> + Send + 'a,
{
    type Fut = Fut;
    fn cb(self, key: &'a str, param: &'a P) -> Self::Fut {
        self(key, param)
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

impl<'a, F, T, R, Fut> MultiCallback<'a, T, R> for F
where
    F: Fn(T) -> Fut + Sync + Send + 'a,
    T: 'a,
    R: 'a,
    Fut: Future<Output = R> + Send + 'a,
{
    type Fut = Fut;
    fn cb(&self, db: T) -> Self::Fut {
        self(db)
    }
}

impl<'a, F, T, R> BoxedMultiCallback<'a, T, R> for OutputBoxer<F>
where
    F: MultiCallback<'a, T, R>,
    R: 'a,
{
    type Fut = BotDbFuture<'a, R>;
    fn cb_boxed(&self, db: T) -> Self::Fut {
        self.0.cb(db).boxed()
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

pub trait CacheMissCallback<'a, V>: Send + Sync {
    type Fut: Future<Output = Result<V>> + Send + 'a;
    fn cb(&self, key: &'a str, val: V) -> Self::Fut;
}

impl<'a, F, V, Fut> CacheMissCallback<'a, V> for F
where
    F: Fn(&'a str, V) -> Fut + Sync + Send + 'a,
    V: Serialize + 'a,
    Fut: Future<Output = Result<V>> + Send + 'a,
{
    type Fut = Fut;
    fn cb(&self, key: &'a str, val: V) -> Self::Fut {
        self(key, val)
    }
}
