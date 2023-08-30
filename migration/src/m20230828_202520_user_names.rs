use bot_impl::persist::core::users;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(users::Entity)
                    .add_column(ColumnDef::new(users::Column::FirstName).text().default(""))
                    .add_column(ColumnDef::new(users::Column::LastName).text())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(users::Entity)
                    .drop_column(users::Column::FirstName)
                    .drop_column(users::Column::LastName)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
