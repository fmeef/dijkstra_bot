use crate::persist::redis::{random_key, scope_key_by_chatuser, RedisStr};
use crate::persist::Result;
use crate::statics::{DB, REDIS};
use crate::tg::client::TgClient;
use crate::util::error::BotError;
use crate::{persist::migrate::ManagerHelper, tg::dialog::Conversation};
use anyhow::anyhow;
use grammers_client::types::media::Sticker;
use grammers_client::types::{CallbackQuery, Chat, InlineQuery, Media, Message, Update, User};
use lazy_static::lazy_static;
use redis::AsyncCommands;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
use sea_schema::migration::{MigrationName, MigrationTrait};

use self::entities::tags::ModelRedis;

// redis keys
const KEY_TYPE_TAG: &str = "wc:tag";
const KEY_TYPE_STICKER_ID: &str = "wc:stickerid";
const KEY_TYPE_STICKER_NAME: &str = "wc:stickername";

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

fn upload_sticker_conversation(message: &Message) -> Result<Conversation> {
    let mut conversation = Conversation::new(
        UPLOAD_CMD.to_string(),
        STATE_START.to_string(),
        message.chat().id(),
        message
            .sender()
            .ok_or_else(|| BotError::new("message has no sender"))?
            .id(),
    )?;
    let start_state = conversation.get_start()?.state_id;
    let name_state = conversation.add_state(STATE_NAME);
    let state_tags = conversation.add_state(STATE_TAGS);
    let state_done = conversation.add_state(STATE_DONE);

    conversation.add_transition(start_state, name_state, TRANSITION_NAME);
    conversation.add_transition(name_state, state_tags, TRANSITION_TAG);
    conversation.add_transition(state_tags, state_tags, TRANSITION_MORETAG);
    conversation.add_transition(state_tags, state_done, TRANSITION_DONE);

    Ok(conversation)
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
            #[sea_orm(primary_key, auto_increment = true)]
            pub id: i64,
            #[sea_orm(column_type = "Text")]
            pub sticker_id: i64,
            pub owner_id: i64,
            #[sea_orm(column_type = "Text")]
            pub tag: String,
        }

        #[derive(DeriveIntoActiveModel, Serialize, Deserialize)]
        pub struct ModelRedis {
            pub sticker_id: i64,
            pub owner_id: i64,
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
            #[sea_orm(primary_key, auto_increment = false)]
            pub unique_id: i64,
            pub owner_id: i64,
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

pub async fn handle_update(_client: TgClient, update: &Update) {
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

async fn conv_start(conversation: Conversation, message: Message) -> Result<Conversation> {
    if let Some(Media::Sticker(Sticker { document, .. })) = message.media() {
        let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
        REDIS
            .pipe(|p| {
                p.set(&key, document.id());
                p.del(&KEY_TYPE_TAG)
            })
            .await?;
        conversation.transition(TRANSITION_NAME).await?;
        Ok(conversation)
    } else {
        Err(anyhow!(BotError::new("Send a sticker")))
    }
}

async fn conv_name(conversation: Conversation, message: Message) -> Result<Conversation> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    REDIS.pipe(|p| p.set(&key, message.text())).await?;
    conversation.transition(TRANSITION_TAG).await?;
    Ok(conversation)
}

async fn conv_moretags(conversation: Conversation, message: Message) -> Result<Conversation> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
    let namekey = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    let taglist = scope_key_by_chatuser(&KEY_TYPE_TAG, &message)?;

    let sticker_id: i64 = REDIS.pipe(|p| p.get(&key)).await?;

    if let Some(Chat::User(user)) = message.sender() {
        if message.text() == "/done" {
            let stickername: String = REDIS.pipe(|p| p.get(&namekey)).await?;

            let tags = REDIS
                .drain_list::<String, ModelRedis>(&taglist)
                .await?
                .into_iter()
                .map(|m| m.into_active_model());

            let sticker = entities::stickers::ActiveModel {
                unique_id: Set(sticker_id),
                owner_id: Set(user.id()),
                chosen_name: Set(Some(stickername)),
            };

            sticker.insert(&*DB).await?;
            entities::tags::Entity::insert_many(tags).exec(&*DB).await?;

            conversation.transition(TRANSITION_DONE).await?;
            Ok(conversation)
        } else {
            let tag = RedisStr::new(&ModelRedis {
                sticker_id,
                owner_id: user.id(),
                tag: message.text().to_owned(),
            })?;

            let randkey = random_key(&"");

            REDIS
                .pipe(|p| {
                    p.atomic();
                    p.set(&randkey, &tag);
                    p.lpush(taglist, randkey)
                })
                .await?;

            conversation.transition(TRANSITION_MORETAG).await?;
            Ok(conversation)
        }
    } else {
        Err(anyhow!(BotError::new("not a user")))
    }
}

async fn print_conversation(
    conversation: Result<Option<Conversation>>,
    message: Message,
) -> Result<Option<Conversation>> {
    match conversation {
        Ok(Some(conversation)) => {
            let text = conversation.get_current_text().await?;
            message.reply(text).await?;
            Ok(Some(conversation))
        }
        Ok(None) => Ok(None),
        Err(err) => {
            let text = format!("Invalid input: {}", err);
            message.reply(text).await?;
            Ok(None)
        }
    }
}

async fn handle_conversation(message: Message) -> Result<()> {
    todo!()
}
