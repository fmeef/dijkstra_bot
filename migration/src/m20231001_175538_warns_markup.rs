use bot_impl::persist::{admin::warns, core::entity};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(warns::Entity)
                    .add_column(ColumnDef::new(warns::Column::EntityId).big_integer())
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(warns::Entity, warns::Column::EntityId)
                    .to(entity::Entity, entity::Column::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .name("warns_entity_fk")
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(warns::Entity)
                    .name("warns_entity_fk")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(warns::Entity)
                    .drop_column(warns::Column::EntityId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
