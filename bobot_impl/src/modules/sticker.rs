use crate::persist::migrate::ManagerHelper;
use crate::tg::client::TgClient;
use sea_schema::migration::{MigrationName, MigrationTrait};

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220412_000001_create_stickertag"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(
        &self,
        manager: &sea_schema::migration::SchemaManager,
    ) -> Result<(), sea_orm::DbErr> {
        manager
            .create_table_auto(entities::stickers::Entity)
            .await?;
        manager.create_table_auto(entities::tags::Entity).await?;
        Ok(())
    }

    async fn down(
        &self,
        manager: &sea_schema::migration::SchemaManager,
    ) -> Result<(), sea_orm::DbErr> {
        manager.drop_table_auto(entities::tags::Entity).await?;
        manager.drop_table_auto(entities::stickers::Entity).await?;
        Ok(())
    }
}

pub mod entities {

    pub mod tags {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "tags")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i64,
            #[sea_orm(column_type = "Text")]
            pub sticker_id: String,
            pub owner_id: i64,
            #[sea_orm(column_type = "Text")]
            pub tag: String,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::stickers::Entity",
                from = "Column::StickerId",
                to = "super::stickers::Column::OwnerId",
                on_update = "NoAction",
                on_delete = "Cascade"
            )]
            Stickers,
        }

        impl Related<super::stickers::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Stickers.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod stickers {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};
        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "stickers")]
        pub struct Model {
            #[sea_orm(column_type = "Text")]
            #[sea_orm(primary_key)]
            pub unique_id: String,
            pub owner_id: i64,
            #[sea_orm(column_type = "Text")]
            pub file_id: String,
            #[sea_orm(column_type = "Text", nullable)]
            pub chosen_name: Option<String>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::tags::Entity")]
            Tags,
        }

        impl Related<super::tags::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Tags.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

pub async fn handle_update(_client: TgClient, _update: &grammers_client::types::update::Update) {}
