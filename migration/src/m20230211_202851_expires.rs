use bobot_impl::persist::admin::actions;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(actions::Entity)
                    .add_column(ColumnDef::new(actions::Column::Expires).date_time().null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(actions::Entity)
                    .drop_column(actions::Column::Expires)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
