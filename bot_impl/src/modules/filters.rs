use std::collections::HashMap;

use crate::metadata::metadata;
use crate::persist::core::button;
use crate::persist::core::media::get_media_type;
use crate::persist::core::media::send_media_reply;
use crate::persist::core::messageentity;
use crate::persist::core::messageentity::EntityWithUser;
use crate::persist::redis::default_cache_query;
use crate::persist::redis::CachedQueryTrait;
use crate::persist::redis::ToRedisStr;
use crate::statics::CONFIG;
use crate::statics::DB;
use crate::statics::REDIS;
use crate::tg::button::InlineKeyboardBuilder;
use crate::tg::command::*;
use crate::tg::markdown::Header;
use crate::tg::markdown::MarkupBuilder;
use crate::tg::markdown::MarkupType;
use crate::tg::permissions::*;
use crate::util::error::Fail;
use crate::util::error::Result;
use crate::util::string::Speak;
use botapi::gen_types::InlineKeyboardButton;
use botapi::gen_types::InlineKeyboardMarkup;
use botapi::gen_types::Message;
use botapi::gen_types::MessageEntity;
use chrono::Duration;
use entities::{filters, triggers};
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use lazy_static::__Deref;
use macros::entity_fmt;

use redis::AsyncCommands;
use sea_orm::entity::ActiveValue;
use sea_orm::sea_query::OnConflict;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::IntoActiveModel;
use sea_orm::QueryFilter;

use sea_orm_migration::{MigrationName, MigrationTrait};

metadata!("Filters",
    r#"
    Respond to keywords with canned messages. This module is guaranteed to cause spam in the support chat
    about how the bot is "alive" or an "AI"
    "#,
    { command = "filter", help = "\\<trigger\\> \\<reply\\>: Trigger a reply when soemone says something" },
    { command = "filters", help = "List all filters" },
    { command = "stop", help = "Stop a filter" },
    { command = "stopall", help = "Stop all filters" }
);

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230127_000001_create_filters"
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
                        .table(filters::Entity)
                        .col(
                            ColumnDef::new(filters::Column::Id)
                                .big_integer()
                                .not_null()
                                .unique_key()
                                .auto_increment(),
                        )
                        .col(
                            ColumnDef::new(filters::Column::Chat)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(filters::Column::Text).text())
                        .col(ColumnDef::new(filters::Column::MediaId).text())
                        .col(
                            ColumnDef::new(filters::Column::MediaType)
                                .integer()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(filters::Column::Id)
                                .primary(),
                        )
                        .index(
                            IndexCreateStatement::new()
                                .col(filters::Column::Chat)
                                .col(filters::Column::Text)
                                .col(filters::Column::MediaId)
                                .unique(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(triggers::Entity)
                        .col(ColumnDef::new(triggers::Column::Trigger).text().not_null())
                        .col(
                            ColumnDef::new(triggers::Column::FilterId)
                                .big_integer()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(triggers::Column::Trigger)
                                .col(triggers::Column::FilterId)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("trigger_id_fk")
                        .from(triggers::Entity, triggers::Column::FilterId)
                        .to(filters::Entity, filters::Column::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .await?;
            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .drop_foreign_key(
                    ForeignKey::drop()
                        .table(triggers::Entity)
                        .name("trigger_id_fk")
                        .to_owned(),
                )
                .await?;
            manager.drop_table_auto(filters::Entity).await?;
            manager.drop_table_auto(triggers::Entity).await?;
            Ok(())
        }
    }

    pub mod triggers {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "triggers")]
        pub struct Model {
            #[sea_orm(primary_key, column_type = "Text")]
            pub trigger: String,
            #[sea_orm(primay_key, unique)]
            pub filter_id: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::filters::Entity",
                from = "Column::FilterId",
                to = "super::filters::Column::Id"
            )]
            Filters,
        }
        impl Related<super::filters::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Filters.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod filters {

        use std::{collections::HashMap, ops::Deref};

        use crate::{
            persist::core::{
                button,
                media::*,
                messageentity::{self, DbMarkupType, EntityWithUser},
            },
            statics::DB,
        };
        use sea_orm::{entity::prelude::*, FromQueryResult, QueryOrder, QuerySelect};
        use sea_query::{IntoCondition, JoinType};
        use serde::{Deserialize, Serialize};

        use super::triggers;

        #[derive(Clone, Debug, Hash, Eq, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "filters")]
        pub struct Model {
            #[sea_orm(primary_key, unique, autoincrement = true)]
            pub id: i64,
            pub chat: i64,
            #[sea_orm(column_type = "Text")]
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: MediaType,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::triggers::Entity")]
            Triggers,
            #[sea_orm(has_many = "crate::persist::core::messageentity::Entity")]
            Entities,
            #[sea_orm(has_many = "crate::persist::core::button::Entity")]
            Buttons,
            #[sea_orm(
                belongs_to = "crate::persist::core::messageentity::Entity",
                from = "Column::Id",
                to = "crate::persist::core::messageentity::Column::OwnerId"
            )]
            Filters,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum MessageEntityRelation {}

        impl Related<super::triggers::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Triggers.def()
            }
        }

        impl Related<crate::persist::core::messageentity::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Entities.def()
            }
        }

        impl Related<Entity> for crate::persist::core::messageentity::Entity {
            fn to() -> RelationDef {
                Relation::Filters.def().rev()
            }
        }

        impl Related<crate::persist::core::button::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Buttons.def()
            }
        }

        impl Related<Entity> for crate::persist::core::button::Entity {
            fn to() -> RelationDef {
                Relation::Buttons.def().rev()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}

        #[derive(FromQueryResult)]
        struct FiltersWithEntities {
            //filter fields
            pub id: Option<i64>,
            pub chat: Option<i64>,
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: Option<MediaType>,

            //button fields
            pub button_id: Option<i64>,
            pub button_text: Option<String>,
            pub callback_data: Option<String>,
            pub button_url: Option<String>,
            pub owner_id: Option<i64>,
            pub pos_x: Option<u32>,
            pub pos_y: Option<u32>,

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

            // trigger fields
            pub trigger: Option<String>,
            pub filter_id: Option<i64>,
        }

        impl FiltersWithEntities {
            fn get(
                self,
            ) -> (
                Option<Model>,
                Option<button::Model>,
                Option<EntityWithUser>,
                Option<triggers::Model>,
            ) {
                let button = if let (
                    Some(button_text),
                    Some(owner_id),
                    Some(button_id),
                    Some(pos_x),
                    Some(pos_y),
                ) = (
                    self.button_text,
                    self.owner_id,
                    self.button_id,
                    self.pos_x,
                    self.pos_y,
                ) {
                    Some(button::Model {
                        button_text,
                        owner_id,
                        button_id,
                        callback_data: self.callback_data,
                        button_url: self.button_url,
                        pos_x,
                        pos_y,
                    })
                } else {
                    None
                };

                let filter = if let (Some(id), Some(chat), Some(media_type)) =
                    (self.id, self.chat, self.media_type)
                {
                    Some(Model {
                        id,
                        chat,
                        media_type,
                        text: self.text,
                        media_id: self.media_id,
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

                let trigger =
                    if let (Some(trigger), Some(filter_id)) = (self.trigger, self.filter_id) {
                        Some(triggers::Model { trigger, filter_id })
                    } else {
                        None
                    };

                (filter, button, entity, trigger)
            }
        }

        pub type FiltersMap = HashMap<
            Model,
            (
                Vec<EntityWithUser>,
                Vec<button::Model>,
                Vec<triggers::Model>,
            ),
        >;

        pub async fn get_filters_join<F>(filter: F) -> crate::util::error::Result<FiltersMap>
        where
            F: IntoCondition,
        {
            let res = super::filters::Entity::find()
                .join(JoinType::LeftJoin, Relation::Entities.def())
                .join(JoinType::LeftJoin, Relation::Buttons.def().rev())
                .join(JoinType::LeftJoin, messageentity::Relation::Users.def())
                .filter(filter)
                .order_by_asc(button::Column::PosX)
                .order_by_asc(button::Column::PosY)
                .into_model::<FiltersWithEntities>()
                .all(DB.deref())
                .await?;

            let res = res.into_iter().map(|v| v.get()).fold(
                FiltersMap::new(),
                |mut acc, (filter, button, entity, trigger)| {
                    if let Some(filter) = filter {
                        let (entitylist, buttonlist, triggerlist) = acc
                            .entry(filter)
                            .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new()));

                        if let Some(button) = button {
                            buttonlist.push(button);
                        }

                        if let Some(entity) = entity {
                            entitylist.push(entity);
                        }

                        if let Some(trigger) = trigger {
                            triggerlist.push(trigger);
                        }
                    }
                    acc
                },
            );

            // let mut res = FiltersMap::new();
            // res.insert(
            //     Model {
            //         id: 0,
            //         chat: 0,
            //         text: None,
            //         media_id: None,
            //         media_type: MediaType::Text,
            //     },
            //     (vec![], vec![], vec![]),
            // );
            Ok(res)
        }
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

fn get_filter_key(message: &Message, id: i64) -> String {
    format!("filter:{}:{}", message.get_chat().get_id(), id)
}

fn get_filter_hash_key(message: &Message) -> String {
    format!("fcache:{}", message.get_chat().get_id())
}

async fn delete_trigger(message: &Message, trigger: &str) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let trigger = &trigger.to_lowercase();
    let hash_key = get_filter_hash_key(message);
    let key: Option<i64> = REDIS
        .query(|mut q| async move {
            let id: Option<i64> = q.hdel(&hash_key, trigger).await?;
            if let Some(id) = id {
                let key = get_filter_key(message, id);
                q.del(&key).await?;
                Ok(Some(id))
            } else {
                Ok(None)
            }
        })
        .await?;
    if let Some(id) = key {
        triggers::Entity::delete_many()
            .filter(
                triggers::Column::FilterId
                    .eq(id)
                    .and(triggers::Column::Trigger.eq(trigger.as_str())),
            )
            .exec(DB.deref())
            .await?;
    } else {
        let filters = triggers::Entity::find()
            .find_with_related(filters::Entity)
            .filter(
                filters::Column::Chat
                    .eq(message.get_chat().get_id())
                    .and(triggers::Column::Trigger.eq(trigger.as_str())),
            )
            .all(DB.deref())
            .await?;

        for (f, _) in filters {
            triggers::Entity::delete_many()
                .filter(
                    triggers::Column::Trigger
                        .eq(f.trigger)
                        .and(triggers::Column::FilterId.eq(f.filter_id)),
                )
                .exec(DB.deref())
                .await?;
        }
    }
    message.speak("Filter stopped").await?;
    Ok(())
}

async fn get_filter(
    message: &Message,
    id: i64,
) -> Result<Option<(filters::Model, Vec<MessageEntity>, InlineKeyboardMarkup)>> {
    default_cache_query(
        |_, _| async move {
            let map = filters::get_filters_join(filters::Column::Id.eq(id))
                .await?
                .into_iter()
                .map(|(filter, (entity, button, _))| {
                    (
                        filter,
                        entity
                            .into_iter()
                            .map(|e| e.get())
                            .map(|(e, u)| e.to_entity(u))
                            .collect(),
                        get_markup_for_buttons(button),
                    )
                })
                .next();
            Ok(map)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&get_filter_key(message, id), &())
    .await
}

fn get_markup_for_buttons(button: Vec<button::Model>) -> InlineKeyboardMarkup {
    button
        .into_iter()
        .fold(InlineKeyboardBuilder::default(), |mut acc, b| {
            let v = acc.get();
            let x = b.pos_x as usize;
            let y = b.pos_y as usize;
            if let Some(ve) = v.get_mut(b.pos_y as usize) {
                ve.insert(x, b.to_button());
            } else {
                let mut ve = Vec::new();
                ve.insert(x, b.to_button());
                v.insert(y, ve);
            }
            acc
        })
        .build()
}

async fn search_cache(
    message: &Message,
    text: &str,
) -> Result<Option<(filters::Model, Vec<MessageEntity>, InlineKeyboardMarkup)>> {
    update_cache_from_db(message).await?;
    let hash_key = get_filter_hash_key(message);
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, i64)> = q.hscan(&hash_key).await?;
            while let Some((key, item)) = iter.next_item().await {
                let t = text.to_lowercase();
                if let Some(mut idx) = t.find(&key) {
                    if idx == 0 && idx + key.len() == text.len() {
                        return get_filter(message, item).await;
                    } else {
                        if idx == 0 {
                            idx = 1;
                        }
                        let keylen = if key.len() + 1 < text.len() {
                            key.len() + idx
                        } else {
                            text.len() - 1
                        };
                        let ws = &text[idx - 1..keylen];
                        if ws.starts_with(|c: char| c.is_whitespace())
                            || ws.ends_with(|c: char| c.is_whitespace())
                        {
                            return get_filter(message, item).await;
                        }
                    }
                }
            }
            Ok(None)
        })
        .await
}

async fn update_cache_from_db(message: &Message) -> Result<()> {
    let hash_key = get_filter_hash_key(message);
    if !REDIS.sq(|q| q.exists(&hash_key)).await? {
        let res = filters::get_filters_join(filters::Column::Chat.eq(message.get_chat().get_id()))
            .await?;

        REDIS
            .try_pipe(|p| {
                p.hset(&hash_key, "empty", 0);
                for (filter, (entities, buttons, triggers)) in res.into_iter() {
                    let key = get_filter_key(message, filter.id);
                    let kb = get_markup_for_buttons(buttons);
                    let entities = entities
                        .into_iter()
                        .map(|v| v.get())
                        .map(|(k, v)| k.to_entity(v))
                        .collect_vec();
                    p.set(&key, (&filter, entities, kb).to_redis()?)
                        .expire(&key, CONFIG.timing.cache_timeout);
                    for trigger in triggers.iter() {
                        p.hset(&hash_key, trigger.trigger.to_owned(), filter.id)
                            .expire(&hash_key, CONFIG.timing.cache_timeout);
                    }
                }
                Ok(p)
            })
            .await?;
    }
    Ok(())
}

async fn command_filter<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
    let cmd = MarkupBuilder::from_murkdown(args.text).await?;

    let (body, entities, buttons, header, footer) = cmd.build_filter();
    let filters = match header.ok_or_else(|| ctx.fail_err("Header missing from filter command"))? {
        Header::List(st) => st,
        Header::Arg(st) => vec![st],
    };

    let filters = filters.iter().map(|v| v.as_str()).collect::<Vec<&str>>();

    let (f, message) = if let Some(message) = message.get_reply_to_message_ref() {
        (message.get_text().map(|v| v.into_owned()), message)
    } else {
        (Some(body), message)
    };

    let (id, media_type) = get_media_type(message)?;
    let model = filters::ActiveModel {
        id: ActiveValue::NotSet,
        chat: ActiveValue::Set(message.get_chat().get_id()),
        text: ActiveValue::Set(f),
        media_id: ActiveValue::Set(id),
        media_type: ActiveValue::Set(media_type),
    };

    let model = filters::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([
                filters::Column::Text,
                filters::Column::Chat,
                filters::Column::MediaId,
            ])
            .update_columns([
                filters::Column::Text,
                filters::Column::Chat,
                filters::Column::MediaId,
                filters::Column::MediaType,
            ])
            .to_owned(),
        )
        .exec_with_returning(DB.deref())
        .await?;
    let triggers = filters
        .iter()
        .map(|v| v.to_lowercase())
        .collect::<Vec<String>>();
    triggers::Entity::insert_many(
        triggers
            .iter()
            .map(|v| {
                triggers::Model {
                    trigger: (*v).to_owned(),
                    filter_id: model.id,
                }
                .into_active_model()
            })
            .collect::<Vec<triggers::ActiveModel>>(),
    )
    .on_conflict(
        OnConflict::columns([triggers::Column::Trigger, triggers::Column::FilterId])
            .update_columns([triggers::Column::Trigger, triggers::Column::FilterId])
            .to_owned(),
    )
    .exec(DB.deref())
    .await?;
    let id = model.id.clone();
    let r = (&model, &entities, &buttons).to_redis()?;

    let entities: Vec<messageentity::Model> = stream::iter(entities)
        .then(|v| async move { messageentity::Model::from_entity(v, id).await })
        .try_collect()
        .await?;

    if entities.len() > 0 {
        messageentity::Entity::insert_many(
            entities
                .into_iter()
                .map(|v| v.into_active_model())
                .collect::<Vec<messageentity::ActiveModel>>(),
        )
        .on_conflict(
            OnConflict::columns([
                messageentity::Column::TgType,
                messageentity::Column::Offset,
                messageentity::Column::Length,
                messageentity::Column::OwnerId,
            ])
            .update_columns([
                messageentity::Column::Url,
                messageentity::Column::User,
                messageentity::Column::Language,
                messageentity::Column::EmojiId,
            ])
            .to_owned(),
        )
        .exec_with_returning(DB.deref())
        .await?;
    }
    let hash_key = get_filter_hash_key(message);
    REDIS
        .pipe(|p| {
            for trigger in triggers {
                p.hset(&hash_key, trigger, id);
            }
            p
        })
        .await?;
    let key = get_filter_key(message, id);
    REDIS.pipe(|q| q.set(&key, r)).await?;
    let filters_fmt = [""].into_iter().chain(filters.into_iter()).join("\n - ");
    let text = MarkupType::Code.text(&filters_fmt);

    message
        .get_chat()
        .speak_fmt(entity_fmt!(ctx, "addfilter", text))
        .await?;
    Ok(())
}

async fn handle_trigger(message: &Message) -> Result<()> {
    if let Some(text) = message.get_text() {
        if let Some((res, extra_entities, extra_buttons)) = search_cache(message, &text).await? {
            send_media_reply(
                message,
                res.media_type,
                res.text,
                res.media_id,
                Some(extra_entities),
                |_, _| async move { Ok(()) }.boxed(),
            )
            .await?;
        }
    }
    Ok(())
}

async fn list_triggers(message: &Message) -> Result<()> {
    let hash_key = get_filter_hash_key(message);
    update_cache_from_db(message).await?;
    let res: Option<HashMap<String, i64>> = REDIS.sq(|q| q.hgetall(&hash_key)).await?;
    if let Some(map) = res {
        let vals = map
            .into_iter()
            .map(|(key, _)| format!("\t- {}", key))
            .collect_vec()
            .join("\n");
        message.reply(format!("Found filters:\n{}", vals)).await?;
    } else {
        message.reply("No filters found!").await?;
    }
    Ok(())
}

async fn stopall(message: &Message) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    filters::Entity::delete_many()
        .filter(filters::Column::Chat.eq(message.get_chat().get_id()))
        .exec(DB.deref())
        .await?;

    let key = get_filter_hash_key(message);
    REDIS.sq(|q| q.del(&key)).await?;
    message.reply("Stopped all filters").await?;
    Ok(())
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "filter" => command_filter(ctx, &args).await?,
            "stop" => delete_trigger(message, args.text).await?,
            "filters" => list_triggers(message).await?,
            "stopall" => stopall(message).await?,
            _ => handle_trigger(message).await?,
        };
    } else if let Ok(message) = ctx.message() {
        handle_trigger(message).await?;
    }

    Ok(())
}

pub async fn handle_update(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}

#[allow(unused)]
mod test {
    use super::*;

    #[test]
    fn parse_cmd2() {
        let cmd = "(fme, fmoo,  cry) menhera";
        let lexer = Lexer::new(cmd);
        let mut parser = Parser::new();
        for token in lexer.all_tokens() {
            println!("token {:?}", token);
            parser.parse(token).unwrap();
        }
        parser.end_of_input().unwrap();
    }

    #[test]
    fn parse_whitespace() {
        let cmd = "fmef menhera";
        let lexer = Lexer::new(cmd);
        let mut parser = Parser::new();
        for token in lexer.all_tokens() {
            println!("token {:?}", token);
            parser.parse(token).unwrap();
        }
        let out = parser.end_of_input().unwrap();
        if let Header::Arg(h) = out.header {
            assert_eq!(h.as_str(), "fmef");
        } else {
            assert!(false);
        }
    }

    #[test]
    fn parse_quote() {
        let cmd = "\"thing manuy\" menhera";
        let lexer = Lexer::new(cmd);
        let mut parser = Parser::new();
        for token in lexer.all_tokens() {
            println!("token {:?}", token);
            parser.parse(token).unwrap();
        }
        let out = parser.end_of_input().unwrap();
        if let Header::Arg(h) = out.header {
            assert_eq!(h.as_str(), "thing manuy");
        } else {
            assert!(false);
        }
    }
}
