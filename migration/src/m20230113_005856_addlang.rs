use dijkstra::persist::core::dialogs;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .add_column(
                        ColumnDef::new(dialogs::Column::ChatType)
                            .string()
                            .not_null(),
                    )
                    .add_column(
                        ColumnDef::new_with_type(
                            dialogs::Column::Language,
                            dijkstra::util::string::Lang::column_type(),
                        )
                        .default(dijkstra::util::string::Lang::En)
                        .not_null(),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::WarnLimit)
                            .integer()
                            .not_null(),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::ActionType)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .drop_column(dialogs::Column::Language)
                    .drop_column(dialogs::Column::ChatType)
                    .drop_column(dialogs::Column::WarnLimit)
                    .drop_column(dialogs::Column::ActionType)
                    .to_owned(),
            )
            .await
    }
}
