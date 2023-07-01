use bot_impl::persist::{core::rules, migrate::ManagerHelper};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(rules::Entity)
                    .col(
                        ColumnDef::new(rules::Column::ChatId)
                            .big_integer()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(rules::Column::Text).text())
                    .col(ColumnDef::new(rules::Column::MediaId).text())
                    .col(
                        ColumnDef::new(rules::Column::MediaType)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(rules::Column::Private)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(rules::Column::ButtonName)
                            .text()
                            .not_null()
                            .default("Rules"),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table_auto(rules::Entity).await
    }
}
