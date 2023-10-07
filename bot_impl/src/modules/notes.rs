use std::collections::BTreeMap;

use crate::metadata::metadata;
use crate::persist::redis::{CachedQuery, CachedQueryTrait, RedisCache, RedisStr};
use crate::statics::{CONFIG, DB, REDIS, TG};

use crate::tg::admin_helpers::IntoChatUser;
use crate::tg::button::{InlineKeyboardBuilder, OnPush};
use crate::tg::command::{
    get_content, handle_deep_link, Cmd, Context, InputType, TextArg, TextArgs,
};

use crate::tg::markdown::{button_deeplink_key, get_markup_for_buttons, MarkupBuilder};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::Username;
use crate::util::error::{BotError, Fail, Result};
use crate::util::string::Speak;
use ::sea_orm_migration::prelude::*;
use botapi::gen_types::{CallbackQuery, Message, MessageEntity};
use futures::future::BoxFuture;
use futures::FutureExt;
use itertools::Itertools;
use lazy_static::__Deref;
use macros::lang_fmt;
use redis::AsyncCommands;
use sea_orm::{ColumnTrait, EntityTrait};

use crate::persist::core::{entity, media::*};

use self::entities::notes;
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

struct EntityInDb;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230117_000001_create_notes"
    }
}

impl MigrationName for EntityInDb {
    fn name(&self) -> &str {
        "m202300118_00002_entity_in_db_notes"
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

    #[async_trait::async_trait]
    impl MigrationTrait for super::EntityInDb {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(notes::Entity)
                        .add_column(ColumnDef::new(notes::Column::EntityId).big_integer())
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(notes::Entity)
                        .drop_column(notes::Column::EntityId)
                        .to_owned(),
                )
                .await?;

            Ok(())
        }
    }

    pub mod notes {
        use std::{collections::HashMap, ops::Deref};

        use crate::{
            persist::core::{
                button, entity,
                media::*,
                messageentity::{self, DbMarkupType, EntityWithUser},
                users,
            },
            statics::DB,
        };

        use sea_orm::{entity::prelude::*, FromQueryResult, QueryOrder, QuerySelect};
        use sea_query::{IntoCondition, JoinType};
        use serde::{Deserialize, Serialize};
        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, Eq, Hash)]
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
            pub entity_id: Option<i64>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "crate::persist::core::entity::Entity",
                from = "Column::EntityId",
                to = "crate::persist::core::entity::Column::Id"
            )]
            Entities,
        }

        impl Related<crate::persist::core::entity::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Entities.def()
            }
        }

        impl Related<Entity> for crate::persist::core::entity::Entity {
            fn to() -> RelationDef {
                Relation::Entities.def().rev()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}

        #[derive(FromQueryResult)]
        struct FiltersWithEntities {
            //filter fields
            pub name: Option<String>,
            pub chat: Option<i64>,
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: Option<MediaType>,
            pub protect: Option<bool>,
            pub entity_id: Option<i64>,

            // button fields
            pub button_text: Option<String>,
            pub callback_data: Option<String>,
            pub button_url: Option<String>,
            pub owner_id: Option<i64>,
            pub pos_x: Option<i32>,
            pub pos_y: Option<i32>,
            pub b_owner_id: Option<i64>,
            pub raw_text: Option<String>,

            // entity fields
            pub tg_type: Option<DbMarkupType>,
            pub offset: Option<i64>,
            pub length: Option<i64>,
            pub url: Option<String>,
            pub user: Option<i64>,
            pub language: Option<String>,
            pub emoji_id: Option<String>,

            // user fields
            pub user_id: Option<i64>,
            pub first_name: Option<String>,
            pub last_name: Option<String>,
            pub username: Option<String>,
            pub is_bot: Option<bool>,
        }

        impl FiltersWithEntities {
            fn get(self) -> (Option<Model>, Option<button::Model>, Option<EntityWithUser>) {
                let button = if let (Some(button_text), Some(owner_id), Some(pos_x), Some(pos_y)) =
                    (self.button_text, self.b_owner_id, self.pos_x, self.pos_y)
                {
                    Some(button::Model {
                        button_text,
                        owner_id: Some(owner_id),
                        callback_data: self.callback_data,
                        button_url: self.button_url,
                        pos_x,
                        pos_y,
                        raw_text: self.raw_text,
                    })
                } else {
                    None
                };

                let filter = if let (Some(name), Some(chat), Some(media_type), Some(protect)) =
                    (self.name, self.chat, self.media_type, self.protect)
                {
                    Some(Model {
                        name,
                        chat,
                        media_type,
                        text: self.text,
                        media_id: self.media_id,
                        protect,
                        entity_id: self.entity_id,
                    })
                } else {
                    None
                };

                let entity = if let (Some(tg_type), Some(offset), Some(length), Some(owner_id)) =
                    (self.tg_type, self.offset, self.length, self.owner_id)
                {
                    Some(EntityWithUser {
                        tg_type,
                        offset,
                        length,
                        url: self.url,
                        language: self.language,
                        emoji_id: self.emoji_id,
                        user: self.user,
                        owner_id,
                        user_id: self.user_id,
                        first_name: self.first_name,
                        last_name: self.last_name,
                        username: self.username,
                        is_bot: self.is_bot,
                    })
                } else {
                    None
                };

                (filter, button, entity)
            }
        }

        pub type FiltersMap = HashMap<Model, (Vec<EntityWithUser>, Vec<button::Model>)>;

        pub async fn get_filters_join<F>(filter: F) -> crate::util::error::Result<FiltersMap>
        where
            F: IntoCondition,
        {
            let res = Entity::find()
                .select_only()
                .columns([
                    Column::Name,
                    Column::Chat,
                    Column::Text,
                    Column::MediaId,
                    Column::MediaType,
                    Column::EntityId,
                    Column::Protect,
                ])
                .columns([
                    messageentity::Column::TgType,
                    messageentity::Column::Offset,
                    messageentity::Column::Length,
                    messageentity::Column::Url,
                    messageentity::Column::User,
                    messageentity::Column::Language,
                    messageentity::Column::EmojiId,
                    messageentity::Column::OwnerId,
                ])
                .columns([
                    button::Column::ButtonText,
                    button::Column::CallbackData,
                    button::Column::ButtonUrl,
                    button::Column::PosX,
                    button::Column::PosY,
                    button::Column::RawText,
                ])
                .column_as(button::Column::OwnerId, "b_owner_id")
                .columns([
                    users::Column::UserId,
                    users::Column::FirstName,
                    users::Column::LastName,
                    users::Column::Username,
                    users::Column::IsBot,
                ])
                .join(JoinType::LeftJoin, Relation::Entities.def())
                .join(JoinType::LeftJoin, entity::Relation::EntitiesRev.def())
                .join(JoinType::LeftJoin, entity::Relation::ButtonsRev.def())
                .join(JoinType::LeftJoin, messageentity::Relation::Users.def())
                .filter(filter)
                .order_by_asc(button::Column::PosX)
                .order_by_asc(button::Column::PosY)
                .into_model::<FiltersWithEntities>()
                .all(DB.deref())
                .await?;

            let res = res.into_iter().map(|v| v.get()).fold(
                FiltersMap::new(),
                |mut acc, (filter, button, entity)| {
                    if let Some(filter) = filter {
                        let (entitylist, buttonlist) = acc
                            .entry(filter)
                            .or_insert_with(|| (Vec::new(), Vec::new()));

                        if let Some(button) = button {
                            buttonlist.push(button);
                        }

                        if let Some(entity) = entity {
                            entitylist.push(entity);
                        }
                    }
                    acc
                },
            );

            //            log::info!("got {:?} filters from db", res);
            Ok(res)
        }
    }
}

async fn get_model<'a>(
    message: &'a Message,
    args: &'a TextArgs<'a>,
) -> Result<entities::notes::Model> {
    let input_type = get_content(message, args)?;
    let res = match input_type {
        InputType::Reply(name, text, message) => {
            let chatuser = message.get_chatuser();
            let (media_id, media_type) = get_media_type(message)?;
            let text = text
                .map(|t| Some(t))
                .unwrap_or_else(|| message.get_caption_ref());
            let (text, entity_id) = if let Some(text) = text {
                let extra = message.get_entities().map(|v| v.into_owned());

                let md = MarkupBuilder::new(extra)
                    .chatuser(chatuser.as_ref())
                    .filling(false)
                    .header(false)
                    .set_text(text.to_owned());
                let (text, entities, buttons) = md.build_murkdown().await?;
                let entity_id = entity::insert(DB.deref(), &entities, buttons).await?;
                (Some(text), Some(entity_id))
            } else {
                (None, None)
            };
            entities::notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text,
                media_id,
                media_type,
                protect: false,
                entity_id,
            }
        }

        InputType::Command(name, content, message) => {
            let (media_id, media_type) = get_media_type(message)?;
            let chatuser = message.get_chatuser();
            let content = content
                .map(|t| Some(t))
                .unwrap_or_else(|| message.get_caption_ref());

            let (text, entity_id) = if let Some(text) = content {
                log::info!("content {}", text);

                let extra = message.get_entities().map(|v| v.into_owned());

                let md = MarkupBuilder::new(extra)
                    .chatuser(chatuser.as_ref())
                    .filling(false)
                    .header(false)
                    .set_text(text.to_owned());
                let (text, entities, buttons) = md.build_murkdown().await?;
                let entity_id = entity::insert(DB.deref(), &entities, buttons).await?;
                (Some(text), Some(entity_id))
            } else {
                (None, None)
            };
            entities::notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text,
                media_id,
                media_type,
                protect: false,
                entity_id,
            }
        }
    };

    Ok(res)
}
pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration), Box::new(EntityInDb)]
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "save" => save(ctx, &args).await,
            "get" => get(ctx).await,
            "delete" => delete(message, args).await,
            "notes" => list_notes(ctx).await,
            "start" => {
                let note: Option<(i64, String)> =
                    handle_deep_link(ctx, |k| button_deeplink_key(k)).await?;
                if let Some((chat, note)) = note {
                    log::info!("handling note deep link {} {}", chat, note);
                    print_chat(ctx, note, chat).await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }?;
    }
    Ok(())
}

fn handle_transition<'a>(
    ctx: &'a Context,
    chat: i64,
    note: String,
    callback: CallbackQuery,
) -> BoxFuture<'a, Result<()>> {
    async move {
        log::info!("current note: {}", note);
        if let Some((note, extra_entities, extra_buttons)) = get_note_by_name(note, chat).await? {
            let c = ctx.clone();
            SendMediaReply::new(ctx, note.media_type)
                .button_callback(move |note, button| {
                    let c = c.clone();
                    async move {
                        log::info!("next notes: {}", note);
                        button.on_push(move |b| async move {
                            TG.client
                                .build_answer_callback_query(b.get_id_ref())
                                .build()
                                .await?;

                            handle_transition(&c, chat, note, b).await?;
                            Ok(())
                        });

                        Ok(())
                    }
                    .boxed()
                })
                .text(note.text)
                .media_id(note.media_id)
                .extra_entities(extra_entities)
                .buttons(extra_buttons)
                .edit_media_reply_chatuser(
                    callback
                        .get_message_ref()
                        .ok_or_else(|| BotError::Generic("message missing".to_owned()))?,
                )
                .await?;
        } else {
            log::warn!("note missing!");
        }

        Ok(())
    }
    .boxed()
}

async fn print_note(
    ctx: &Context,
    note: entities::notes::Model,
    entities: Vec<MessageEntity>,
    buttons: Option<InlineKeyboardBuilder>,
    note_chat: i64,
) -> Result<()> {
    let c = ctx.clone();
    SendMediaReply::new(ctx, note.media_type)
        .button_callback(move |note, button| {
            let c = c.clone();
            async move {
                button.on_push(move |b| async move {
                    TG.client
                        .build_answer_callback_query(b.get_id_ref())
                        .build()
                        .await?;
                    handle_transition(&c, note_chat, note, b).await?;
                    Ok(())
                });

                Ok(())
            }
            .boxed()
        })
        .text(note.text)
        .media_id(note.media_id)
        .extra_entities(entities)
        .buttons(buttons)
        .send_media_reply()
        .await?;
    Ok(())
}

async fn print(message: &Context, name: String) -> Result<()> {
    print_chat(message, name, message.message()?.get_chat().get_id()).await
}

async fn print_chat(ctx: &Context, name: String, chat: i64) -> Result<()> {
    if let Some((note, entities, buttons)) = get_note_by_name(name, chat).await? {
        print_note(ctx, note, entities, buttons, chat).await?;
        Ok(())
    } else {
        ctx.fail("Note not found")
    }
}

async fn get<'a>(ctx: &Context) -> Result<()> {
    ctx.is_group_or_die().await?;
    let message = ctx.message()?;
    if let Some(&Cmd { ref args, .. }) = ctx.cmd() {
        let name = match args.args.first() {
            Some(TextArg::Arg(name)) => Some(name),
            Some(TextArg::Quote(name)) => Some(name),
            _ => None,
        };
        if let Some(name) = name {
            print(ctx, (*name).to_owned()).await
        } else {
            Err(BotError::speak(
                "missing note name, try again weenie",
                message.get_chat().get_id(),
            ))
        }
    } else {
        Err(BotError::Generic("not a command".to_owned()))
    }
}

#[inline(always)]
fn get_hash_key(chat: i64) -> String {
    format!("ncch:{}", chat)
}

async fn refresh_notes(
    chat: i64,
) -> Result<
    BTreeMap<
        String,
        (
            entities::notes::Model,
            Vec<MessageEntity>,
            Option<InlineKeyboardBuilder>,
        ),
    >,
> {
    let hash_key = get_hash_key(chat);
    let (exists, notes): (bool, BTreeMap<String, RedisStr>) = REDIS
        .pipe(|q| q.exists(&hash_key).hgetall(&hash_key))
        .await?;

    if !exists {
        let notes = entities::notes::get_filters_join(entities::notes::Column::Chat.eq(chat))
            .await?
            .into_iter()
            .map(|(note, (entity, button))| {
                (
                    note,
                    entity
                        .into_iter()
                        .map(|e| e.get())
                        .map(|(e, u)| e.to_entity(u))
                        .collect(),
                    get_markup_for_buttons(button),
                )
            })
            .collect_vec();
        let st = notes
            .iter()
            .filter_map(|v| {
                if let Some(s) = RedisStr::new(&v).ok() {
                    Some((v.0.name.clone(), s))
                } else {
                    None
                }
            })
            .collect_vec();
        REDIS
            .pipe(|q| {
                if st.len() > 0 {
                    q.hset_multiple(&hash_key, &st.as_slice());
                }
                q.expire(&hash_key, CONFIG.timing.cache_timeout)
            })
            .await?;

        Ok(notes.into_iter().map(|v| (v.0.name.clone(), v)).collect())
    } else {
        Ok(notes
            .into_iter()
            .filter_map(|(n, v)| v.get().ok().map(|v| (n, v)))
            .collect())
    }
}

async fn get_note_by_name(
    name: String,
    chat: i64,
) -> Result<
    Option<(
        entities::notes::Model,
        Vec<MessageEntity>,
        Option<InlineKeyboardBuilder>,
    )>,
> {
    let hash_key = get_hash_key(chat);
    let n = name.clone();
    let note = CachedQuery::new(
        |_, _| async move {
            let res = entities::notes::get_filters_join(
                notes::Column::Name.eq(n).and(notes::Column::Chat.eq(chat)),
            )
            .await?;

            Ok(res
                .into_iter()
                .map(|(note, (entity, button))| {
                    (
                        note,
                        entity
                            .into_iter()
                            .map(|e| e.get())
                            .map(|(e, u)| e.to_entity(u))
                            .collect(),
                        get_markup_for_buttons(button),
                    )
                })
                .next())
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
    message.check_permissions(|p| p.can_change_info).await?;
    let model = get_model(message, args).await?;
    let name = model.name.clone();
    let hash_key = get_hash_key(message.get_chat().get_id());
    REDIS.sq(|q| q.hdel(&hash_key, &model.name)).await?;
    entities::notes::Entity::delete_by_id((model.name, message.get_chat().get_id()))
        .exec(DB.deref())
        .await?;
    message.speak(format!("Deleted note {}", name)).await?;
    Ok(())
}

async fn list_notes(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    let message = ctx.message()?;
    let notes = refresh_notes(message.get_chat().get_id()).await?;
    let m = [lang_fmt!(
        ctx,
        "listnotes",
        message.get_chat().name_humanreadable()
    )]
    .into_iter()
    .chain(notes.iter().map(|(n, _)| format!("- {}", n)))
    .collect::<Vec<String>>()
    .join("\n");
    message.reply(m).await?;
    Ok(())
}

async fn save<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
    let chat = message.get_chat().name_humanreadable();
    let model = get_model(message, args).await?;
    let key = format!("note:{}:{}", message.get_chat().get_id(), model.name);
    log::info!("save key: {}", key);
    let hash_key = get_hash_key(message.get_chat().get_id());
    REDIS.sq(|q| q.del(&hash_key)).await?;
    let name = model.name.clone();
    entities::notes::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::columns([entities::notes::Column::Name, entities::notes::Column::Chat])
                .update_columns([
                    entities::notes::Column::Text,
                    entities::notes::Column::MediaId,
                    entities::notes::Column::MediaType,
                    entities::notes::Column::Protect,
                    entities::notes::Column::EntityId,
                ])
                .to_owned(),
        )
        .exec(DB.deref())
        .await?;

    message
        .speak(lang_fmt!(ctx, "savednote", name, chat))
        .await?;
    Ok(())
}

pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    if let Ok(message) = cmd.message() {
        if let Some(text) = message.get_text_ref() {
            if text.starts_with("#") && text.len() > 1 {
                let tail = &text[1..];
                print(cmd, tail.to_owned()).await?;
            }
        }
    }
    handle_command(cmd).await?;

    Ok(())
}
