use bot_impl::persist::{core::taint, migrate::ManagerHelper};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(taint::Entity)
                    .col(ColumnDef::new(taint::Column::MediaId).text().primary_key())
                    .col(
                        ColumnDef::new(taint::Column::MediaType)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table_auto(taint::Entity).await
    }
}
