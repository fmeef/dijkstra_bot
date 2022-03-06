use async_trait::async_trait;
use sea_schema::migration::*;
use sea_schema::sea_query::{IntoTableRef, Table};

pub async fn create_table<'a, T>(manager: &SchemaManager<'a>, table: T) -> Result<(), DbErr>
where
    T: IntoTableRef,
{
    manager
        .create_table(Table::create().table(table).if_not_exists().to_owned())
        .await
}

pub async fn remove_table<'a, T>(manager: &SchemaManager<'a>, table: T) -> Result<(), DbErr>
where
    T: IntoTableRef + 'static,
{
    manager
        .drop_table(Table::drop().table(table).if_exists().to_owned())
        .await
}

#[async_trait]
pub trait ManagerHelper {
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
