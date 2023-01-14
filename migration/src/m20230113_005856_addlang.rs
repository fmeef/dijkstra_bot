use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(bobot_impl::persist::core::dialogs::Entity)
                    .add_column(
                        ColumnDef::new_with_type(
                            bobot_impl::persist::core::dialogs::Column::Language,
                            bobot_impl::util::string::Lang::column_type(),
                        )
                        .default(bobot_impl::util::string::Lang::En)
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
                    .table(bobot_impl::persist::core::dialogs::Entity)
                    .drop_column(bobot_impl::persist::core::dialogs::Column::Language)
                    .to_owned(),
            )
            .await
    }
}
