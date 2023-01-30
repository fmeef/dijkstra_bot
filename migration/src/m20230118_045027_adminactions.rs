use bobot_impl::persist::admin::actions;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                        ColumnDef::new(actions::Column::Warns)
                            .integer()
                            .not_null()
                            .default(0),
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
            .await
    }
}
