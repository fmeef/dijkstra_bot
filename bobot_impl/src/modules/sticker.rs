use crate::tg::client::TgClient;
use crate::tg::Result;
use crate::{
    persist::migrate::ManagerHelper, tg::dialog::Conversation, tg::dialog::ConversationData,
};
use grammers_client::types::media::Sticker;
use grammers_client::types::{CallbackQuery, InlineQuery, Media, Message};
use grammers_client::Update;
use lazy_static::lazy_static;
use sea_schema::migration::{MigrationName, MigrationTrait};

// redis keys
const KEY_TYPE_TAG: &str = "wc:tag";
const KEY_TYPE_STICKER: &str = "wc:sticker";

// conversation state machine globals
const UPLOAD_CMD: &str = "/upload";
const TRANSITION_NAME: &str = "stickername";
const TRANSITION_DONE: &str = "stickerdone";
const TRANSITION_TAG: &str = "stickertag";
const TRANSITION_MORETAG: &str = "stickermoretag";
const STATE_START: &str = "Send a sticker to upload";
const STATE_NAME: &str = "Send a name for this sticker";
const STATE_TAGS: &str = "Send tags for this sticker, one at a time. Send /done to stop";
const STATE_DONE: &str = "Successfully uploaded sticker";

fn upload_sticker_conversation() -> Result<Conversation> {
    let mut conversation =
        ConversationData::new_anonymous(UPLOAD_CMD.to_string(), STATE_START.to_string())?;
    let start_state = conversation.get_start()?.state_id;
    let name_state = conversation.add_state(STATE_NAME);
    let state_tags = conversation.add_state(STATE_TAGS);
    let state_done = conversation.add_state(STATE_DONE);

    conversation.add_transition(start_state, name_state, TRANSITION_NAME);
    conversation.add_transition(name_state, state_tags, TRANSITION_TAG);
    conversation.add_transition(state_tags, state_tags, TRANSITION_MORETAG);
    conversation.add_transition(state_tags, state_done, TRANSITION_DONE);

    Ok(conversation.build())
}

lazy_static! {
    static ref STICKER_UPLOAD_CONV_TEMPLATE: Conversation = upload_sticker_conversation().unwrap();
}

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
    ) -> std::result::Result<(), sea_orm::DbErr> {
        manager
            .create_table_auto(entities::stickers::Entity)
            .await?;
        manager.create_table_auto(entities::tags::Entity).await?;
        Ok(())
    }

    async fn down(
        &self,
        manager: &sea_schema::migration::SchemaManager,
    ) -> std::result::Result<(), sea_orm::DbErr> {
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
            pub sticker_id: u64,
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
            pub unique_id: i64,
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

pub async fn handle_update(_client: TgClient, update: &grammers_client::types::update::Update) {
    match update {
        Update::NewMessage(ref message) => {
            if let Some(Media::Sticker(ref document)) = message.media() {
                println!("sticker id {}", document.document.id());
            }
        }
        Update::CallbackQuery(ref _foo) => (),
        Update::InlineQuery(ref _foo) => (),
        _ => (),
    };
}
