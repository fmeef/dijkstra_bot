use std::collections::BTreeMap;

use crate::metadata::metadata;
use crate::persist::redis::{CachedQuery, CachedQueryTrait, RedisCache, RedisStr};
use crate::statics::{CONFIG, DB, REDIS, TG};
use crate::tg::admin_helpers::is_group_or_die;

use crate::tg::button::OnPush;
use crate::tg::command::{get_content, handle_deep_link, Context, InputType, TextArg, TextArgs};

use crate::tg::markdown::button_deeplink_key;
use crate::util::string::Speak;
use ::sea_orm_migration::prelude::*;
use futures::future::BoxFuture;
use futures::FutureExt;
use itertools::Itertools;
use redis::AsyncCommands;

use lazy_static::__Deref;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::util::error::{BotError, Result};
use botapi::gen_types::{CallbackQuery, Message, UpdateExt};

use crate::persist::core::media::*;
metadata!("Notes",
    r#"
    Easily store and retrive text, media, and other content by keywords. 
    Useful for storing answers to often asked questions or searching uploaded media.     
    "#,
    { sub = "Examples", content = r#"teset"# },
    { command = "save", help = "Saves a note" },
    { command = "get", help = "Get a note" },
    { command = "delete", help = "Delete a note" },
    { command = "notes", help = "List all notes for the current chat"}
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
    if let Some((cmd, _, args, message, _)) = ctx.cmd() {
        match cmd {
            "save" => save(message, &args).await,
            "get" => get(message, &args).await,
            "delete" => delete(message, args).await,
            "notes" => list_notes(message).await,
            "start" => {
                let note: Option<(i64, String)> =
                    handle_deep_link(ctx, |k| button_deeplink_key(k)).await?;
                if let Some((chat, note)) = note {
                    print_chat(message, note, chat).await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }?;
    }
    Ok(())
}

fn handle_transition<'a>(b: CallbackQuery, chat: i64, note: String) -> BoxFuture<'a, Result<()>> {
    async move {
        log::info!("current note: {}", note);
        if let (Some(note), Some(message)) = (get_note_by_name(note, chat).await?, b.get_message())
        {
            edit_media_reply_chatuser(
                &message,
                note.media_type,
                note.text,
                note.media_id,
                |note, button| {
                    async move {
                        log::info!("next notes: {}", note);
                        button.on_push(move |b| async move {
                            TG.client
                                .build_answer_callback_query(b.get_id_ref())
                                .build()
                                .await?;

                            handle_transition(b, chat, note).await?;
                            Ok(())
                        });
                        Ok(())
                    }
                    .boxed()
                },
            )
            .await?;
        } else {
            log::warn!("note missing!");
        }

        Ok(())
    }
    .boxed()
}

async fn print_note(message: &Message, note: entities::notes::Model, note_chat: i64) -> Result<()> {
    send_media_reply(
        message,
        note.media_type,
        note.text,
        note.media_id,
        |note, button| {
            async move {
                button.on_push(move |b| async move {
                    TG.client
                        .build_answer_callback_query(b.get_id_ref())
                        .build()
                        .await?;
                    handle_transition(b, note_chat, note).await?;
                    Ok(())
                });
                Ok(())
            }
            .boxed()
        },
    )
    .await?;
    Ok(())
}

async fn print(message: &Message, name: String) -> Result<()> {
    print_chat(message, name, message.get_chat_ref().get_id()).await
}

async fn print_chat(message: &Message, name: String, chat: i64) -> Result<()> {
    if let Some(note) = get_note_by_name(name, chat).await? {
        print_note(message, note, chat).await?;
        Ok(())
    } else {
        Err(BotError::speak(
            "note not found",
            message.get_chat_ref().get_id(),
        ))
    }
}

async fn get<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;
    let name = match args.args.first() {
        Some(TextArg::Arg(name)) => Some(name),
        Some(TextArg::Quote(name)) => Some(name),
        _ => None,
    };
    if let Some(name) = name {
        print(message, (*name).to_owned()).await
    } else {
        Err(BotError::speak(
            "missing note name, try again weenie",
            message.get_chat().get_id(),
        ))
    }
}

#[inline(always)]
fn get_hash_key(chat: i64) -> String {
    format!("ncch:{}", chat)
}

async fn refresh_notes(chat: i64) -> Result<BTreeMap<String, entities::notes::Model>> {
    let hash_key = get_hash_key(chat);
    let (exists, notes): (bool, BTreeMap<String, RedisStr>) = REDIS
        .pipe(|q| q.exists(&hash_key).hgetall(&hash_key))
        .await?;

    if !exists {
        let notes = entities::notes::Entity::find()
            .filter(entities::notes::Column::Chat.eq(chat))
            .all(DB.deref())
            .await?;
        let st = notes
            .iter()
            .filter_map(|v| {
                if let Some(s) = RedisStr::new(&v).ok() {
                    Some((v.name.clone(), s))
                } else {
                    None
                }
            })
            .collect_vec();
        REDIS
            .pipe(|q| {
                q.hset_multiple(&hash_key, st.as_slice())
                    .expire(&hash_key, CONFIG.timing.cache_timeout)
            })
            .await?;

        Ok(notes
            .into_iter()
            .map(|v| (v.name.clone(), v))
            .collect::<BTreeMap<String, entities::notes::Model>>())
    } else {
        Ok(notes
            .into_iter()
            .filter_map(|(n, v)| v.get().ok().map(|v| (n, v)))
            .collect())
    }
}

async fn get_note_by_name(name: String, chat: i64) -> Result<Option<entities::notes::Model>> {
    let hash_key = get_hash_key(chat);
    let n = name.clone();
    let note = CachedQuery::new(
        |_, _| async move {
            let res = entities::notes::Entity::find_by_id((n, chat))
                .one(DB.deref().deref())
                .await?;

            Ok(res)
        },
        |key, _| async move {
            let (exists, key, _): (bool, Option<RedisStr>, ()) = REDIS
                .pipe(|q| {
                    q.exists(&hash_key)
                        .hget(&hash_key, key)
                        .expire(&hash_key, CONFIG.timing.cache_timeout)
                })
                .await?;

            let res = if let Some(key) = key {
                Some(key.get()?)
            } else {
                None
            };

            Ok((exists, res))
        },
        |_, value| async move {
            refresh_notes(chat).await?;
            Ok(value)
        },
    )
    .query(&name, &())
    .await?;
    Ok(note)
}

async fn delete<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;
    let model = get_model(message, args)?;
    let name = model.name.clone();
    let hash_key = get_hash_key(message.get_chat().get_id());
    REDIS.sq(|q| q.hdel(&hash_key, &model.name)).await?;
    entities::notes::Entity::delete_by_id((model.name, message.get_chat().get_id()))
        .exec(DB.deref().deref())
        .await?;
    message.speak(format!("Deleted note {}", name)).await?;
    Ok(())
}

async fn list_notes(message: &Message) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;
    let notes = refresh_notes(message.get_chat().get_id()).await?;
    let m = [String::from("Notes for {chatname}")]
        .into_iter()
        .chain(notes.iter().map(|(n, _)| format!("- {}", n)))
        .collect::<Vec<String>>()
        .join("\n");
    message.reply(m).await?;
    Ok(())
}

async fn save<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;

    let model = get_model(message, args)?;
    let key = format!("note:{}:{}", message.get_chat().get_id(), model.name);
    log::info!("save key: {}", key);
    let hash_key = get_hash_key(message.get_chat().get_id());
    let rs = RedisStr::new(&model)?;
    REDIS.sq(|q| q.hset(&hash_key, &model.name, rs)).await?;
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

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    if let Some(cmd) = cmd {
        if let Some(text) = cmd.message.get_text_ref() {
            if text.starts_with("#") && text.len() > 1 {
                let tail = &text[1..];
                print(&cmd.message, tail.to_owned()).await?;
            }
        }
        handle_command(cmd).await?;
    }
    Ok(())
}
