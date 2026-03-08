use dijkstra::persist::core::users;
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // If this fails its telegram's fault
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(users::Entity)
                    .modify_column(
                        ColumnDef::new(users::Column::Username)
                            .text()
                            .null()
                            .unique_key(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_username_unique")
                    .col(users::Column::Username)
                    .unique()
                    .table(users::Entity)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(users::Entity)
                    .modify_column(ColumnDef::new(users::Column::Username).text().null())
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                IndexDropStatement::new()
                    .name("idx_username_unique")
                    .table(users::Entity)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Post {
    Table,
    Id,
    Title,
    Text,
}
