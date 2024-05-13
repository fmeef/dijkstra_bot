use dijkstra::persist::core::*;
use dijkstra::persist::migrate::ManagerHelper;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .create_table(
                Table::create()
                    .table(users::Entity)
                    .col(
                        ColumnDef::new(users::Column::UserId)
                            .big_integer()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(users::Column::Username).string().null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts
        manager
            .drop_table_auto(dijkstra::persist::core::users::Entity)
            .await?;
        Ok(())
    }
}
