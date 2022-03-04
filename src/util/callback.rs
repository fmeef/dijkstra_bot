use futures::{future::BoxFuture, Future, FutureExt};

use super::error::BotError;

pub type BotDbResult<T> = std::result::Result<T, BotError>;
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

pub trait BoxedSingleCallback<'a, T, U>: Send + Sync {
    type Fut: 'a + Future<Output = U> + Send;
    fn cb_boxed(self: Box<Self>, val: &'a T) -> Self::Fut;
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

// type erasure on the future
struct OutputBoxer<F>(F);
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

// type erasure on the function, with the future already type-erased
pub struct BoxedDbCallback<T, U>(
    Box<dyn for<'a> BoxedSingleCallback<'a, T, U, Fut = BotDbFuture<'a, U>>>,
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

impl<'a, T, U> SingleCallback<'a, T, U> for BoxedDbCallback<T, U>
where
    U: 'a,
{
    type Fut = BotDbFuture<'a, U>;

    fn cb(self, val: &'a T) -> Self::Fut {
        self.0.cb_boxed(val)
    }
}
