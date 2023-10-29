use bot_impl::persist::core::notes;
use sea_orm_migration::prelude::*;
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(notes::Entity)
                    .add_column(ColumnDef::new(notes::Column::EntityId).big_integer())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(notes::Entity)
                    .drop_column(notes::Column::EntityId)
                    .to_owned(),
            )
            .await
    }
}

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m202300118_00002_entity_in_db_notes"
    }
}
