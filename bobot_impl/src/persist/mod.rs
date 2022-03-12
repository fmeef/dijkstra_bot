pub(crate) type Result<T> = anyhow::Result<T>;

pub mod core;
pub mod migrate;
#[allow(dead_code)]
pub(crate) mod redis;
