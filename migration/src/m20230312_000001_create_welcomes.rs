use bot_impl::persist::{core::welcomes, migrate::ManagerHelper};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(welcomes::Entity)
                    .col(
                        ColumnDef::new(welcomes::Column::Chat)
                            .big_integer()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(welcomes::Column::Text).text())
                    .col(ColumnDef::new(welcomes::Column::MediaId).text())
                    .col(ColumnDef::new(welcomes::Column::MediaType).integer())
                    .col(ColumnDef::new(welcomes::Column::GoodbyeText).text())
                    .col(ColumnDef::new(welcomes::Column::GoodbyeMediaId).text())
                    .col(ColumnDef::new(welcomes::Column::GoodbyeMediaType).integer())
                    .col(
                        ColumnDef::new(welcomes::Column::Enabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table_auto(welcomes::Entity).await?;

        Ok(())
    }
}
