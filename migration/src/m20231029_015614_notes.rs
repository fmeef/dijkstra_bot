use bot_impl::persist::{core::notes, migrate::ManagerHelper};
use sea_orm_migration::prelude::*;

pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(notes::Entity)
                    .col(ColumnDef::new(notes::Column::Name).text())
                    .col(ColumnDef::new(notes::Column::Chat).big_integer())
                    .col(ColumnDef::new(notes::Column::Text).text())
                    .col(ColumnDef::new(notes::Column::MediaId).text())
                    .col(
                        ColumnDef::new(notes::Column::MediaType)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(notes::Column::Protect)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .primary_key(
                        IndexCreateStatement::new()
                            .col(notes::Column::Name)
                            .col(notes::Column::Chat)
                            .primary(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table_auto(notes::Entity).await
    }
}

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230117_000001_create_notes"
    }
}
