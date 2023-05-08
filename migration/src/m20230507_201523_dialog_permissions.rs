use bobot_impl::persist::core::dialogs;
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendMessages)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendAudio)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendVideo)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendPhoto)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendDocument)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendVoiceNote)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendVideoNote)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendPoll)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(
                        ColumnDef::new(dialogs::Column::CanSendOther)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .alter_table(
                Table::alter()
                    .table(dialogs::Entity)
                    .drop_column(dialogs::Column::CanSendMessages)
                    .drop_column(dialogs::Column::CanSendAudio)
                    .drop_column(dialogs::Column::CanSendVideo)
                    .drop_column(dialogs::Column::CanSendPhoto)
                    .drop_column(dialogs::Column::CanSendDocument)
                    .drop_column(dialogs::Column::CanSendVoiceNote)
                    .drop_column(dialogs::Column::CanSendVideoNote)
                    .drop_column(dialogs::Column::CanSendPoll)
                    .drop_column(dialogs::Column::CanSendOther)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
