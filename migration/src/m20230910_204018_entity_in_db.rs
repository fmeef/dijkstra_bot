use bot_impl::persist::{
    core::{messageentity, users},
    migrate::ManagerHelper,
};
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
                    .add_column(
                        ColumnDef::new(users::Column::IsBot)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                TableCreateStatement::new()
                    .table(messageentity::Entity)
                    .col(
                        ColumnDef::new(messageentity::Column::TgType)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(messageentity::Column::Offset)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(messageentity::Column::Length)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(messageentity::Column::Url).text())
                    .col(
                        ColumnDef::new(messageentity::Column::User)
                            .big_integer()
                            .null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(messageentity::Column::Language).text())
                    .col(ColumnDef::new(messageentity::Column::EmojiId).text())
                    .col(
                        ColumnDef::new(messageentity::Column::OwnerId)
                            .big_integer()
                            .unique_key(),
                    )
                    .index(
                        IndexCreateStatement::new()
                            .col(messageentity::Column::TgType)
                            .col(messageentity::Column::Offset)
                            .col(messageentity::Column::Length)
                            .col(messageentity::Column::OwnerId)
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .name("entity_user_fk")
                    .from(messageentity::Entity, messageentity::Column::User)
                    .to(users::Entity, users::Column::UserId)
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
                    .drop_column(users::Column::IsBot)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .name("entity_user_fk")
                    .table(messageentity::Entity)
                    .to_owned(),
            )
            .await?;

        manager.drop_table_auto(messageentity::Entity).await?;
        Ok(())
    }
}
