use async_trait::async_trait;

pub(crate) type Result<T> = anyhow::Result<T>;

#[allow(dead_code)]
pub(crate) mod redis;

#[async_trait]
pub trait DbTable<T> {
    async fn insert(&self, pool: &T, wrapper: Option<String>) -> Result<()>;
}
