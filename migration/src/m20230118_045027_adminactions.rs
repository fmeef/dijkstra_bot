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
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(actions::Column::ChatId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
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
                        ColumnDef::new(actions::Column::IsMuted)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(actions::Column::Action).integer())
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
