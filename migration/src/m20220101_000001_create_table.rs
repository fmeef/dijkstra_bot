use dijkstra::persist::core::*;
use dijkstra::persist::migrate::ManagerHelper;
use sea_orm_migration::prelude::*;

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
            .create_table(
                Table::create()
                    .table(chat_members::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(chat_members::Column::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(chat_members::Column::ChatId)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // not a composite primary key, but as close as we can get
        manager
            .create_index(
                Index::create()
                    .name("chatuser")
                    .table(chat_members::Entity)
                    .col(chat_members::Column::UserId)
                    .col(chat_members::Column::ChatId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(conversation_states::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(conversation_states::Column::StateId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(conversation_states::Column::Parent)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(conversation_states::Column::Content)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(conversation_states::Column::StartFor).uuid())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(conversation_transitions::Entity)
                    .col(
                        ColumnDef::new(conversation_transitions::Column::TransitionId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(conversation_transitions::Column::StartState)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(conversation_transitions::Column::EndState)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(conversation_transitions::Column::Triggerphrase)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(conversations::Entity)
                    .col(
                        ColumnDef::new(conversations::Column::ConversationId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(conversations::Column::Triggerphrase)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(conversations::Column::ChatId).big_integer())
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(
                        conversation_transitions::Entity,
                        conversation_transitions::Column::StartState,
                    )
                    .to(
                        conversation_states::Entity,
                        conversation_states::Column::StateId,
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(
                        conversation_transitions::Entity,
                        conversation_transitions::Column::EndState,
                    )
                    .to(
                        conversation_states::Entity,
                        conversation_states::Column::StateId,
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(
                        conversation_states::Entity,
                        conversation_states::Column::Parent,
                    )
                    .to(conversations::Entity, conversations::Column::ConversationId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(dialogs::Entity)
                    .col(
                        ColumnDef::new(dialogs::Column::ChatId)
                            .big_integer()
                            .primary_key(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table_auto(dijkstra::persist::core::dialogs::Entity)
            .await?;
        manager
            .drop_table_auto(dijkstra::persist::core::conversation_transitions::Entity)
            .await?;
        manager
            .drop_table_auto(dijkstra::persist::core::conversation_states::Entity)
            .await?;
        manager
            .drop_table_auto(dijkstra::persist::core::conversations::Entity)
            .await?;
        manager
            .drop_table_auto(dijkstra::persist::core::chat_members::Entity)
            .await?;
        Ok(())
    }
}
