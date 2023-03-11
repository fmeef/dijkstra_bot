use crate::metadata::metadata;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::DB;
use crate::tg::command::{single_arg, Context, TextArg, TextArgs};

use crate::util::string::Speak;
use ::sea_orm_migration::prelude::*;
use chrono::Duration;

use lazy_static::__Deref;
use sea_orm::EntityTrait;

use crate::util::error::{BotError, Result};
use botapi::gen_types::{Message, UpdateExt};

use crate::persist::core::media::*;

metadata!("Notes",
    r#"
    Easily store and retrive text, media, and other content by keywords. 
    Useful for storing answers to often asked questions or searching uploaded media.     
    "#,
    { command = "save", help = "Saves a note" },
    { command = "get", help = "Get a note" }
);

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230117_000001_create_notes"
    }
}

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;
    #[async_trait::async_trait]
    impl MigrationTrait for super::Migration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(notes::Entity)
                        .col(ColumnDef::new(notes::Column::Name).text())
                        .col(ColumnDef::new(notes::Column::Chat).big_integer())
                        .col(ColumnDef::new(notes::Column::Text).text())
                        .col(ColumnDef::new(notes::Column::MediaId).text())
                        .col(
                            ColumnDef::new(notes::Column::MediaType)
                                .integer()
                                .not_null(),
                        )
                        .col(
                            ColumnDef::new(notes::Column::Protect)
                                .boolean()
                                .not_null()
                                .default(false),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(notes::Column::Name)
                                .col(notes::Column::Chat)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager.drop_table_auto(notes::Entity).await?;

            Ok(())
        }
    }
    pub mod notes {
        use crate::persist::core::media::*;
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};
        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "notes")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub name: String,
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(column_type = "Text")]
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: MediaType,
            #[sea_orm(default = false)]
            pub protect: bool,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::notes::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

enum InputType<'a> {
    Reply(&'a str, Option<&'a str>, &'a Message),
    Command(&'a str, Option<&'a str>, &'a Message),
}

fn get_input_type<'a>(
    message: &'a Message,
    textargs: &'a TextArgs<'a>,
    name: &'a str,
    end: usize,
) -> InputType<'a> {
    log::info!("get:{}", textargs.text);
    if let Some(reply) = message.get_reply_to_message_ref() {
        InputType::Reply(name, reply.get_text_ref(), reply)
    } else {
        let tail = &textargs.text[end..];
        InputType::Command(name, Some(tail), message)
    }
}

fn get_content<'a>(message: &'a Message, textargs: &'a TextArgs<'a>) -> Result<InputType<'a>> {
    match single_arg(textargs.text) {
        Some((TextArg::Arg(name), _, end)) => Ok(get_input_type(message, textargs, name, end)),
        Some((TextArg::Quote(name), _, end)) => Ok(get_input_type(message, textargs, name, end)),
        _ => Err(BotError::speak(
            "Invalid argument, need to specify name",
            message.get_chat().get_id(),
        )),
    }
}

fn get_model<'a>(message: &'a Message, args: &'a TextArgs<'a>) -> Result<entities::notes::Model> {
    let input_type = get_content(message, args)?;
    let res = match input_type {
        InputType::Reply(name, text, message) => {
            let (media_id, media_type) = get_media_type(message)?;
            entities::notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text: text.map(|t| t.to_owned()),
                media_id,
                media_type,
                protect: false,
            }
        }

        InputType::Command(name, content, message) => {
            let (media_id, media_type) = get_media_type(message)?;
            entities::notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text: content.map(|t| t.to_owned()),
                media_id,
                media_type,
                protect: false,
            }
        }
    };

    Ok(res)
}
pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, _, args, message)) = ctx.cmd() {
        log::info!("admin command {}", cmd);

        match cmd {
            "save" => save(message, &args).await,
            "get" => get(message, &args).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

async fn print_note(message: &Message, note: entities::notes::Model) -> Result<()> {
    send_media_reply(message, note.media_type, note.text, note.media_id).await?;
    Ok(())
}

async fn get<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    let name = match args.args.first() {
        Some(TextArg::Arg(name)) => Some(name),
        Some(TextArg::Quote(name)) => Some(name),
        _ => None,
    };
    if let Some(name) = name {
        let key = format!("note:{}:{}", message.get_chat().get_id(), name);
        log::info!("get key: {}", key);
        let chat = message.get_chat().get_id();
        let name = (*name).to_owned();
        let note = default_cache_query(
            move |_, _| async move {
                let res = entities::notes::Entity::find_by_id((name, chat))
                    .one(DB.deref().deref())
                    .await?;
                Ok(res)
            },
            Duration::days(1),
        )
        .query(&key, &())
        .await?;

        if let Some(note) = note {
            print_note(message, note).await?;
            Ok(())
        } else {
            Err(BotError::speak("note not found", chat))
        }
    } else {
        Err(BotError::speak(
            "missing note name, try again weenie",
            message.get_chat().get_id(),
        ))
    }
}

async fn save<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    let model = get_model(message, args)?;
    let key = format!("note:{}:{}", message.get_chat().get_id(), model.name);
    log::info!("save key: {}", key);
    let name = model.name.clone();
    entities::notes::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::columns([entities::notes::Column::Name, entities::notes::Column::Chat])
                .update_columns([
                    entities::notes::Column::Text,
                    entities::notes::Column::MediaId,
                    entities::notes::Column::MediaType,
                    entities::notes::Column::Protect,
                ])
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;
    message.speak(format!("Saved note {}", name)).await?;
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Context<'a>) -> Result<()> {
    handle_command(cmd).await
}
