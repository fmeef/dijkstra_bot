use super::ManagerHelper;
use sea_schema::migration::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220101_000001_create_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table_auto(entities::core::chat_members::Entity)
            .await?;
        manager
            .create_table_auto(entities::core::conversation_states::Entity)
            .await?;
        manager
            .create_table_auto(entities::core::conversation_transitions::Entity)
            .await?;
        manager
            .create_table_auto(entities::core::conversations::Entity)
            .await?;
        manager
            .create_table_auto(entities::core::dialogs::Entity)
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table_auto(entities::core::dialogs::Entity)
            .await?;
        manager
            .drop_table_auto(entities::core::conversations::Entity)
            .await?;
        manager
            .drop_table_auto(entities::core::conversation_transitions::Entity)
            .await?;
        manager
            .drop_table_auto(entities::core::conversation_states::Entity)
            .await?;
        manager
            .drop_table_auto(entities::core::chat_members::Entity)
            .await?;
        Ok(())
    }
}
