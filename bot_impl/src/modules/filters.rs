use std::collections::HashMap;

use crate::metadata::metadata;
use crate::persist::core::button;
use crate::persist::core::entity;
use crate::persist::core::media::get_media_type;
use crate::persist::core::media::SendMediaReply;
use crate::persist::core::messageentity;
use crate::persist::redis::RedisStr;
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
use botapi::gen_types::InlineKeyboardMarkup;
use botapi::gen_types::Message;
use botapi::gen_types::MessageEntity;
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
struct MigrationEntityInDb;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230127_000001_create_filters"
    }
}

impl MigrationName for MigrationEntityInDb {
    fn name(&self) -> &str {
        "m20230127_000002_filters_entity_in_db"
    }
}

pub mod entities {
    use crate::persist::{core::entity, migrate::ManagerHelper};
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

    #[async_trait::async_trait]
    impl MigrationTrait for super::MigrationEntityInDb {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(filters::Entity)
                        .add_column(ColumnDef::new(filters::Column::EntityId).big_integer())
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("filters_entity_fk")
                        .from(filters::Entity, filters::Column::EntityId)
                        .to(entity::Entity, entity::Column::Id)
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
                        .table(entity::Entity)
                        .name("filters_entity_fk")
                        .to_owned(),
                )
                .await?;

            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(filters::Entity)
                        .drop_column(filters::Column::EntityId)
                        .to_owned(),
                )
                .await?;
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

        use super::triggers;
        use crate::{
            persist::core::{
                button, entity,
                media::*,
                messageentity::{self, DbMarkupType, EntityWithUser},
            },
            statics::DB,
        };
        use sea_orm::{entity::prelude::*, FromQueryResult, QueryOrder, QuerySelect};
        use sea_query::{IntoCondition, JoinType};
        use serde::{Deserialize, Serialize};

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
            pub entity_id: Option<i64>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::triggers::Entity")]
            Triggers,
            #[sea_orm(
                belongs_to = "crate::persist::core::entity::Entity",
                from = "Column::EntityId",
                to = "crate::persist::core::entity::Column::Id"
            )]
            Entities,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum MessageEntityRelation {}

        impl Related<super::triggers::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Triggers.def()
            }
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
            pub id: Option<i64>,
            pub chat: Option<i64>,
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: Option<MediaType>,
            pub entity_id: Option<i64>,

            //button fields
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
                let button = if let (Some(button_text), Some(owner_id), Some(pos_x), Some(pos_y)) =
                    (self.button_text, self.owner_id, self.pos_x, self.pos_y)
                {
                    Some(button::Model {
                        button_text,
                        owner_id,
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
                .select_only()
                .columns([
                    Column::Id,
                    Column::Chat,
                    Column::Text,
                    Column::MediaId,
                    Column::MediaType,
                    Column::EntityId,
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
                .columns([triggers::Column::Trigger, triggers::Column::FilterId])
                .columns([
                    button::Column::ButtonText,
                    button::Column::CallbackData,
                    button::Column::ButtonUrl,
                ])
                .join(JoinType::LeftJoin, Relation::Entities.def())
                .join(JoinType::LeftJoin, entity::Relation::EntitiesRev.def())
                .join(JoinType::LeftJoin, entity::Relation::ButtonsRev.def())
                .join(JoinType::LeftJoin, messageentity::Relation::Users.def())
                .join(JoinType::LeftJoin, Relation::Triggers.def())
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

            log::info!("got {} filters from db", res.len());
            Ok(res)
        }
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration), Box::new(MigrationEntityInDb)]
}

fn get_filter_key(message: &Message, id: i64) -> String {
    format!("filter:{}:{}", message.get_chat().get_id(), id)
}

fn get_filter_hash_key(message: &Message) -> String {
    format!("fcache:{}", message.get_chat().get_id())
}

async fn delete_trigger(ctx: &Context, trigger: &str) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
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
) -> Result<
    Option<(
        filters::Model,
        Vec<MessageEntity>,
        Option<InlineKeyboardMarkup>,
    )>,
> {
    let filter_key = get_filter_key(message, id);
    let v: Option<RedisStr> = REDIS.sq(|q| q.get(&filter_key)).await?;
    if let Some(v) = v {
        log::info!("cache hit");
        Ok(v.get()?)
    } else {
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

        if let Some(ref map) = map {
            REDIS
                .try_pipe(|p| {
                    Ok(p.set(&filter_key, map.to_redis()?)
                        .expire(&filter_key, CONFIG.timing.cache_timeout))
                })
                .await?;
        }
        Ok(map)
    }
}

fn get_markup_for_buttons(button: Vec<button::Model>) -> Option<InlineKeyboardMarkup> {
    if button.len() == 0 {
        None
    } else {
        let b = button
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
            .build();
        Some(b)
    }
}

async fn search_cache(
    message: &Message,
    text: &str,
) -> Result<
    Option<(
        filters::Model,
        Vec<MessageEntity>,
        Option<InlineKeyboardMarkup>,
    )>,
> {
    update_cache_from_db(message).await?;
    let hash_key = get_filter_hash_key(message);
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, i64)> = q.hscan(&hash_key).await?;
            while let Some((key, item)) = iter.next_item().await {
                log::info!("search cache {}", item);
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
                    log::info!("triggers {}", triggers.len());
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

    let (body, entities, buttons, header, _) = cmd.build_filter();
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
        entity_id: ActiveValue::NotSet,
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

    let key = get_filter_key(message, model.id);
    let model_id = model.id;

    let r = (&model, &entities, &buttons).to_redis()?;

    let entity_id = if let Some(id) = model.entity_id {
        id
    } else {
        let id = entity::Entity::insert(entity::ActiveModel {
            id: ActiveValue::NotSet,
        })
        .exec_with_returning(DB.deref())
        .await?
        .id;
        let mut active = model.into_active_model();
        active.entity_id = ActiveValue::Set(Some(id));
        filters::Entity::update(active).exec(DB.deref()).await?;
        id
    };

    let entities: Vec<messageentity::Model> = stream::iter(entities)
        .then(|v| async move { messageentity::Model::from_entity(v, entity_id).await })
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
    let buttons = buttons
        .get_inline_keyboard_ref()
        .into_iter()
        .enumerate()
        .flat_map(|(pos_y, list)| {
            list.into_iter().enumerate().map(move |(pos_x, button)| {
                button::Model::from_button(pos_x as u32, pos_y as u32, button, entity_id)
            })
        })
        .collect_vec();
    if buttons.len() > 0 {
        button::Entity::insert_many(buttons)
            .on_conflict(
                OnConflict::columns([
                    button::Column::OwnerId,
                    button::Column::PosX,
                    button::Column::PosY,
                ])
                .update_columns([
                    button::Column::ButtonText,
                    button::Column::CallbackData,
                    button::Column::ButtonUrl,
                ])
                .to_owned(),
            )
            .exec(DB.deref())
            .await?;
    }
    let hash_key = get_filter_hash_key(message);
    REDIS
        .pipe(|p| {
            for trigger in triggers {
                p.hset(&hash_key, trigger, model_id);
            }
            p
        })
        .await?;

    REDIS.pipe(|q| q.set(&key, r)).await?;
    let filters_fmt = [""].into_iter().chain(filters.into_iter()).join("\n - ");
    let text = MarkupType::Code.text(&filters_fmt);

    message
        .get_chat()
        .speak_fmt(entity_fmt!(ctx, "addfilter", text))
        .await?;
    Ok(())
}

async fn handle_trigger(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    if let Some(text) = message.get_text() {
        if let Some((res, extra_entities, extra_buttons)) = search_cache(message, &text).await? {
            SendMediaReply::new(ctx, res.media_type)
                .button_callback(|_, _| async move { Ok(()) }.boxed())
                .text(res.text)
                .media_id(res.media_id)
                .extra_entities(extra_entities)
                .extra_buttons(extra_buttons)
                .send_media_reply()
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

async fn stopall(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
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
            "stop" => delete_trigger(ctx, args.text).await?,
            "filters" => list_triggers(message).await?,
            "stopall" => stopall(ctx).await?,
            _ => handle_trigger(ctx).await?,
        };
    } else if let Ok(_) = ctx.message() {
        handle_trigger(ctx).await?;
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
