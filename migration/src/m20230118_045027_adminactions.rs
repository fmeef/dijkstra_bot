use bobot_impl::persist::{
    admin::{actions, warns},
    migrate::ManagerHelper,
};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(warns::Entity)
                    .col(
                        ColumnDef::new(warns::Column::Id)
                            .big_integer()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(
                        ColumnDef::new(warns::Column::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(warns::Column::ChatId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(warns::Column::Reason).string())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(actions::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(actions::Column::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(actions::Column::ChatId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(actions::Column::Pending)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(actions::Column::IsBanned)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(actions::Column::CanSendMessages)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(actions::Column::CanSendMedia)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(actions::Column::CanSendPoll)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(actions::Column::CanSendOther)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(actions::Column::Action).integer())
                    .primary_key(
                        IndexCreateStatement::new()
                            .col(actions::Column::UserId)
                            .col(actions::Column::ChatId)
                            .primary(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(actions::Entity).to_owned())
            .await?;

        manager.drop_table_auto(warns::Entity).await?;
        Ok(())
    }
}
