use crate::persist::redis::{scope_key_by_chatuser, RedisStr};
use crate::persist::Result;
use crate::statics::{DB, REDIS};
use crate::tg::client::TgClient;
use crate::tg::command::{parse_cmd, Arg};
use crate::tg::dialog::Conversation;
use crate::tg::dialog::{get_conversation, replace_conversation};
use crate::util::error::BotError;
use anyhow::anyhow;
use grammers_client::types::media::Sticker;
use grammers_client::types::{Chat, Media, Message, Update};
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

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use sea_schema::migration::prelude::*;
    #[async_trait::async_trait]
    impl MigrationTrait for super::Migration {
        async fn up(
            &self,
            manager: &sea_schema::migration::SchemaManager,
        ) -> std::result::Result<(), sea_orm::DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(tags::Entity)
                        .col(
                            ColumnDef::new(tags::Column::Id)
                                .big_integer()
                                .primary_key()
                                .auto_increment(),
                        )
                        .col(
                            ColumnDef::new(tags::Column::StickerId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(
                            ColumnDef::new(tags::Column::OwnerId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(tags::Column::Tag).text().not_null())
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(stickers::Entity)
                        .col(
                            ColumnDef::new(stickers::Column::UniqueId)
                                .big_integer()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(stickers::Column::OwnerId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(stickers::Column::ChosenName).text())
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .from(tags::Entity, tags::Column::StickerId)
                        .to(stickers::Entity, stickers::Column::UniqueId)
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(
            &self,
            manager: &sea_schema::migration::SchemaManager,
        ) -> std::result::Result<(), sea_orm::DbErr> {
            manager.drop_table_auto(tags::Entity).await?;
            manager.drop_table_auto(stickers::Entity).await?;
            Ok(())
        }
    }
    pub mod tags {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "tags")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = true)]
            pub id: i64,
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
        pub enum Relation {}

        impl Related<super::stickers::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
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

        impl Model {
            pub fn get_uuid(&self) -> Uuid {
                let mut bytes = Vec::<u8>::with_capacity(16);
                let mut b1 = self.unique_id.to_be_bytes();
                let mut b2 = self.owner_id.to_be_bytes();
                bytes.extend_from_slice(&mut b1);
                bytes.extend_from_slice(&mut b2);
                let bytes: [u8; 16] = bytes.try_into().expect("this should never fail");
                Uuid::from_bytes(bytes)
            }
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
    let res = match update {
        Update::NewMessage(ref message) => handle_command(message).await,
        Update::CallbackQuery(ref _foo) => Ok(()),
        Update::InlineQuery(ref _foo) => Ok(()),
        _ => Ok(()),
    };

    if let Err(err) = res {
        println!("error {}", err);
    }
}

async fn handle_command(message: &Message) -> Result<()> {
    let command = parse_cmd(message.text())?;
    if let Some(Arg::Arg(cmd)) = command.first() {
        println!("command {}", cmd);
        match cmd.as_str() {
            "/upload" => {
                replace_conversation(message, |message| upload_sticker_conversation(message))
                    .await?;
                message.reply(STATE_START).await?;
                println!("handle command {}", message.text());
                handle_conversation(message).await
            }
            "/list" => list_stickers(message).await,
            _ => handle_conversation(message).await,
        }
    } else {
        Err(anyhow!(BotError::new("missing command")))
    }
}

async fn list_stickers(message: &Message) -> Result<()> {
    if let Some(sender) = message.sender() {
        let stickers = entities::stickers::Entity::find()
            .filter(entities::stickers::Column::OwnerId.eq(sender.id()))
            .all(&*DB)
            .await?;
        let stickers = stickers
            .into_iter()
            .fold(String::from("My stickers:"), |mut s, sticker| {
                let default = "Unnamed".to_string();
                let chosenname = sticker.chosen_name.as_ref().unwrap_or(&default);
                s.push_str(format!("\n - {} {}", chosenname, sticker.get_uuid()).as_str());
                s
            });
        message.reply(stickers).await?;
    }
    Ok(())
}

async fn conv_start(conversation: Conversation, message: &Message) -> Result<()> {
    if let Some(Media::Sticker(Sticker { document, .. })) = message.media() {
        let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
        let taglist = scope_key_by_chatuser(&KEY_TYPE_TAG, &message)?;
        REDIS
            .pipe(|p| {
                p.set(&key, document.id());
                p.del(&taglist)
            })
            .await?;
        let text = conversation.transition(TRANSITION_NAME).await?;
        message.reply(text).await?;
        Ok(())
    } else {
        Err(anyhow!(BotError::new("Send a sticker")))
    }
}

async fn conv_name(conversation: Conversation, message: &Message) -> Result<()> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    REDIS.pipe(|p| p.set(&key, message.text())).await?;
    let text = conversation.transition(TRANSITION_TAG).await?;
    message.reply(text).await?;
    Ok(())
}

async fn conv_moretags(conversation: Conversation, message: &Message) -> Result<()> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
    let namekey = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    let taglist = scope_key_by_chatuser(&KEY_TYPE_TAG, &message)?;

    let sticker_id: (i64,) = REDIS.pipe(|p| p.get(&key)).await?;
    let sticker_id = sticker_id.0;
    println!("moretags stickerid: {}", sticker_id);
    if let Some(Chat::User(user)) = message.sender() {
        if message.text() == "/done" {
            let stickername: (String,) = REDIS.pipe(|p| p.get(&namekey)).await?;
            let stickername = stickername.0;

            let tags = REDIS
                .drain_list::<String, ModelRedis>(&taglist)
                .await?
                .into_iter()
                .map(|m| {
                    println!("tag id {}", m.sticker_id);
                    m.into_active_model()
                });

            let sticker = entities::stickers::ActiveModel {
                unique_id: Set(sticker_id),
                owner_id: Set(user.id()),
                chosen_name: Set(Some(stickername)),
            };

            println!("inserting sticker {}", sticker_id);
            sticker.insert(&*DB).await?;

            println!("inserting tags {}", tags.len());
            entities::tags::Entity::insert_many(tags).exec(&*DB).await?;

            let text = conversation.transition(TRANSITION_DONE).await?;
            message.reply(text).await?;
            Ok(())
        } else {
            let tag = RedisStr::new(&ModelRedis {
                sticker_id,
                owner_id: user.id(),
                tag: message.text().to_owned(),
            })?;

            REDIS
                .pipe(|p| {
                    p.atomic();
                    p.lpush(taglist, &tag)
                })
                .await?;

            let text = conversation.transition(TRANSITION_MORETAG).await?;
            message.reply(text).await?;
            Ok(())
        }
    } else {
        Err(anyhow!(BotError::new("not a user")))
    }
}

async fn handle_conversation(message: &Message) -> Result<()> {
    if let Some(conversation) = get_conversation(&message).await? {
        println!("hello conversation");
        if let Err(err) = match conversation.get_current_text().await?.as_str() {
            STATE_START => conv_start(conversation, &message).await,
            STATE_NAME => conv_name(conversation, &message).await,
            STATE_TAGS => conv_moretags(conversation, &message).await,
            _ => return Ok(()),
        } {
            let reply = format!("Error: {}", err);
            message.reply(reply).await?;
        }
    } else {
        println!("nope no conversation for u");
    }
    Ok(())
}
