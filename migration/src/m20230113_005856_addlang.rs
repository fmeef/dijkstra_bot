use bobot_impl::persist::core::dialogs;
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
                            bobot_impl::util::string::Lang::column_type(),
                        )
                        .default(bobot_impl::util::string::Lang::En)
                        .not_null(),
                    )
                    .add_column(ColumnDef::new(dialogs::Column::WarnLimit).integer())
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
                    .to_owned(),
            )
            .await
    }
}
