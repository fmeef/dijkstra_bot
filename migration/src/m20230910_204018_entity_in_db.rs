use bot_impl::persist::{
    core::{button, entity, messageentity, users},
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
                    .table(entity::Entity)
                    .col(
                        ColumnDef::new(entity::Column::Id)
                            .big_integer()
                            .auto_increment()
                            .primary_key(),
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
                            .null(),
                    )
                    .col(ColumnDef::new(messageentity::Column::Language).text())
                    .col(ColumnDef::new(messageentity::Column::EmojiId).text())
                    .col(ColumnDef::new(messageentity::Column::OwnerId).big_integer())
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
            .create_table(
                TableCreateStatement::new()
                    .table(button::Entity)
                    .col(ColumnDef::new(button::Column::ButtonText).text().not_null())
                    .col(ColumnDef::new(button::Column::CallbackData).text())
                    .col(ColumnDef::new(button::Column::ButtonUrl).text())
                    .col(ColumnDef::new(button::Column::PosX).unsigned().not_null())
                    .col(ColumnDef::new(button::Column::PosY).unsigned().not_null())
                    .col(
                        ColumnDef::new(button::Column::OwnerId)
                            .big_integer()
                            .not_null(),
                    )
                    .index(
                        IndexCreateStatement::new()
                            .col(button::Column::OwnerId)
                            .col(button::Column::PosX)
                            .col(button::Column::PosY)
                            .primary(),
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
        manager.drop_table_auto(button::Entity).await?;
        manager.drop_table_auto(entity::Entity).await?;

        Ok(())
    }
}
