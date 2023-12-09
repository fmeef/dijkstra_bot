use dijkstra::persist::admin::approvals;
use dijkstra::persist::migrate::ManagerHelper;
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(approvals::Entity)
                    .col(
                        ColumnDef::new(approvals::Column::Chat)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(approvals::Column::User)
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        IndexCreateStatement::new()
                            .col(approvals::Column::Chat)
                            .col(approvals::Column::User)
                            .primary(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        manager.drop_table_auto(approvals::Entity).await?;
        Ok(())
    }
}
