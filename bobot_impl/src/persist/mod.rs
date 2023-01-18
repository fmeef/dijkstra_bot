pub(crate) type Result<T> = anyhow::Result<T>;

pub mod admin;
pub mod core;
pub mod migrate;
#[allow(dead_code)]
pub(crate) mod redis;
