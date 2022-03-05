use async_trait::async_trait;
pub use sea_schema::migration::*;
use sea_schema::sea_query::{IntoTableRef, Table};

mod m20220101_000001_create_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20220101_000001_create_table::Migration)]
    }
}

pub(crate) async fn create_table<'a, T>(manager: &SchemaManager<'a>, table: T) -> Result<(), DbErr>
where
    T: IntoTableRef,
{
    manager
        .create_table(Table::create().table(table).if_not_exists().to_owned())
        .await
}

pub(crate) async fn remove_table<'a, T>(manager: &SchemaManager<'a>, table: T) -> Result<(), DbErr>
where
    T: IntoTableRef + 'static,
{
    manager
        .drop_table(Table::drop().table(table).if_exists().to_owned())
        .await
}

#[async_trait]
pub(crate) trait ManagerHelper {
    async fn create_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send;

    async fn drop_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send + 'static;
}

#[async_trait]
impl<'a> ManagerHelper for SchemaManager<'a> {
    async fn create_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send,
    {
        create_table(self, table).await
    }

    async fn drop_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send + 'static,
    {
        remove_table(self, table).await
    }
}
