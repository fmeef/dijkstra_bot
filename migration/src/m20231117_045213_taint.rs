use dijkstra::persist::{core::taint, migrate::ManagerHelper};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(taint::Entity)
                    .col(ColumnDef::new(taint::Column::Id).uuid().primary_key())
                    .col(ColumnDef::new(taint::Column::MediaId).text())
                    .col(ColumnDef::new(taint::Column::Scope).text().not_null())
                    .col(ColumnDef::new(taint::Column::Notes).text().null())
                    .col(ColumnDef::new(taint::Column::Details).text().null())
                    .col(ColumnDef::new(taint::Column::Chat).big_integer().not_null())
                    .col(
                        ColumnDef::new(taint::Column::MediaType)
                            .integer()
                            .not_null(),
                    )
                    .index(
                        IndexCreateStatement::new()
                            .col(taint::Column::MediaId)
                            .col(taint::Column::Scope)
                            .col(taint::Column::Chat)
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(taint::Entity)
                    .col(taint::Column::Scope)
                    .name("scope_search_index")
                    .index_type(IndexType::BTree)
                    .to_owned(),
            )
            .await?;

        // manager
        //     .create_table(
        //         Table::create()
        //             .table(taint_chats::Entity)
        //             .col(
        //                 ColumnDef::new(taint_chats::Column::MediaId)
        //                     .text()
        //                     .not_null(),
        //             )
        //             .col(
        //                 ColumnDef::new(taint_chats::Column::Chat)
        //                     .big_integer()
        //                     .not_null(),
        //             )
        //             .index(
        //                 IndexCreateStatement::new()
        //                     .col(taint_chats::Column::MediaId)
        //                     .col(taint_chats::Column::Chat)
        //                     .primary(),
        //             )
        //             .to_owned(),
        //     )
        //     .await?;

        // manager
        //     .create_foreign_key(
        //         ForeignKeyCreateStatement::new()
        //             .from(taint_chats::Entity, taint_chats::Column::MediaId)
        //             .to(taint::Entity, taint::Column::MediaId)
        //             .name("taint_media_id_fk")
        //             .to_owned(),
        //     )
        //     .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // manager
        //     .drop_foreign_key(
        //         ForeignKeyDropStatement::new()
        //             .table(taint_chats::Entity)
        //             .name("taint_media_id_fk")
        //             .to_owned(),
        //     )
        //     .await?;
        // manager.drop_table_auto(taint_chats::Entity).await?;

        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(taint::Entity)
                    .name("scope_search_index")
                    .to_owned(),
            )
            .await?;
        manager.drop_table_auto(taint::Entity).await?;
        Ok(())
    }
}
