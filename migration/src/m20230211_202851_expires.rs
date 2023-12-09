use dijkstra::persist::{
    admin::{
        actions::{self, ActionType},
        warns,
    },
    core::dialogs,
};
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
                    .add_column(
                        ColumnDef::new(actions::Column::Expires)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(warns::Entity)
                    .add_column(
                        ColumnDef::new(warns::Column::Expires)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .add_column(
                        ColumnDef::new(dialogs::Column::WarnTime)
                            .big_integer()
                            .null(),
                    )
                    .modify_column(
                        ColumnDef::new(dialogs::Column::WarnLimit)
                            .integer()
                            .not_null()
                            .default(3),
                    )
                    .modify_column(
                        ColumnDef::new(dialogs::Column::ActionType)
                            .integer()
                            .not_null()
                            .default(ActionType::Mute),
                    )
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

        manager
            .alter_table(
                Table::alter()
                    .table(warns::Entity)
                    .drop_column(warns::Column::Expires)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .drop_column(dialogs::Column::WarnTime)
                    .modify_column(
                        ColumnDef::new(dialogs::Column::WarnLimit)
                            .integer()
                            .not_null(),
                    )
                    .modify_column(
                        ColumnDef::new(dialogs::Column::ActionType)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
