//! Migration helpers for handling migrations from modules

use async_trait::async_trait;
use sea_orm_migration::manager::SchemaManager;
use sea_orm_migration::prelude::*;
use sea_orm_migration::DbErr;

/// Shortcut to drop table if exists
pub async fn remove_table<'a, T>(manager: &SchemaManager<'a>, table: T) -> Result<(), DbErr>
where
    T: IntoTableRef + 'static,
{
    manager
        .drop_table(Table::drop().table(table).if_exists().to_owned())
        .await
}

/// Extension trait to help with dropping tables without boilerplate
#[async_trait]
pub trait ManagerHelper {
    /// Drop table if exists
    async fn drop_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send + 'static;
}

#[async_trait]
impl<'a> ManagerHelper for SchemaManager<'a> {
    async fn drop_table_auto<T>(&self, table: T) -> Result<(), DbErr>
    where
        T: IntoTableRef + Send + 'static,
    {
        remove_table(self, table).await
    }
}
