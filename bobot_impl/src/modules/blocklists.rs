use std::collections::HashMap;

use crate::persist::admin::actions::ActionType;
use crate::persist::redis::default_cache_query;
use crate::persist::redis::CachedQueryTrait;
use crate::persist::redis::RedisCache;
use crate::persist::redis::RedisStr;
use crate::statics::CONFIG;
use crate::statics::DB;
use crate::statics::REDIS;
use crate::tg::admin_helpers::ban;
use crate::tg::admin_helpers::mute;
use crate::tg::admin_helpers::parse_duration_str;
use crate::tg::admin_helpers::warn_ban;
use crate::tg::admin_helpers::warn_mute;
use crate::tg::admin_helpers::warn_shame;
use crate::tg::admin_helpers::warn_user;
use crate::tg::admin_helpers::IsAdmin;
use crate::tg::admin_helpers::IsGroupAdmin;
use crate::tg::command::Command;

use crate::tg::command::TextArgs;

use crate::tg::dialog::dialog_or_default;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::util::error::Result;

use crate::metadata::metadata;
use crate::util::filter::Header;
use crate::util::filter::Lexer;
use crate::util::filter::Parser;

use crate::util::glob::Glob;
use crate::util::string::Speak;
use botapi::gen_types::User;
use botapi::gen_types::{Message, UpdateExt};
use chrono::Duration;
use entities::{blocklists, triggers};
use humantime::format_duration;
use itertools::Itertools;
use lazy_static::__Deref;
use lazy_static::lazy_static;
use redis::AsyncCommands;
use regex::Regex;
use sea_orm::entity::ActiveValue;
use sea_orm::sea_query::OnConflict;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::IntoActiveModel;
use sea_orm::QueryFilter;

use sea_orm_migration::{MigrationName, MigrationTrait};
use wildmatch::WildMatch;
metadata!("Filters",
    { command = "blocklist", help = "<trigger> <reply> {action}: Add a blocklist" },
    { command = "blocklists", help = "List all blocklists" },
    { command = "stopbl", help = "Stop a blocklist by trigger" },
    { command = "stopallbl", help = "Stop all blocklists" }
);

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230222_000001_create_blocklists"
    }
}

pub mod entities {
    use crate::persist::{admin::actions::ActionType, migrate::ManagerHelper};
    use ::sea_orm_migration::prelude::*;
    use chrono::Duration;

    #[async_trait::async_trait]
    impl MigrationTrait for super::Migration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(blocklists::Entity)
                        .col(
                            ColumnDef::new(blocklists::Column::Id)
                                .big_integer()
                                .not_null()
                                .auto_increment()
                                .unique_key(),
                        )
                        .col(
                            ColumnDef::new(blocklists::Column::Chat)
                                .big_integer()
                                .not_null(),
                        )
                        .col(
                            ColumnDef::new(blocklists::Column::Action)
                                .integer()
                                .not_null()
                                .default(ActionType::Mute),
                        )
                        .col(
                            ColumnDef::new(blocklists::Column::Duration)
                                .big_integer()
                                .default(Duration::minutes(3).num_seconds()),
                        )
                        .col(ColumnDef::new(blocklists::Column::Reason).text())
                        .index(
                            IndexCreateStatement::new()
                                .col(blocklists::Column::Chat)
                                .col(blocklists::Column::Action)
                                .col(blocklists::Column::Reason)
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
                            ColumnDef::new(triggers::Column::BlocklistId)
                                .big_integer()
                                .not_null(),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(triggers::Column::Trigger)
                                .col(triggers::Column::BlocklistId)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("blocklist_id_fk")
                        .from(triggers::Entity, triggers::Column::BlocklistId)
                        .to(blocklists::Entity, blocklists::Column::Id)
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
                        .name("blocklist_id_fk")
                        .to_owned(),
                )
                .await?;
            manager.drop_table_auto(blocklists::Entity).await?;
            manager.drop_table_auto(triggers::Entity).await?;
            Ok(())
        }
    }

    pub mod triggers {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "blocklist_triggers")]
        pub struct Model {
            #[sea_orm(primary_key, column_type = "Text")]
            pub trigger: String,
            #[sea_orm(primay_key, unique)]
            pub blocklist_id: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::blocklists::Entity",
                from = "Column::BlocklistId",
                to = "super::blocklists::Column::Chat"
            )]
            Filters,
        }
        impl Related<super::blocklists::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Filters.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod blocklists {

        use crate::persist::admin::actions::ActionType;
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "blocklists")]
        pub struct Model {
            #[sea_orm(primary_key, unique, autoincrement = true)]
            pub id: i64,
            #[sea_orm(primary_key)]
            pub chat: i64,
            pub action: ActionType,
            pub reason: Option<String>,
            pub duration: Option<i64>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::triggers::Entity")]
            Triggers,
        }
        impl Related<super::triggers::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Triggers.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

fn get_blocklist_key(message: &Message, id: i64) -> String {
    format!("blockl:{}:{}", message.get_chat().get_id(), id)
}

fn get_blocklist_hash_key(message: &Message) -> String {
    format!("bcache:{}", message.get_chat().get_id())
}

async fn delete_trigger(message: &Message, trigger: &str) -> Result<()> {
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;
    let trigger = &trigger.to_lowercase();
    let hash_key = get_blocklist_hash_key(message);
    let key: Option<i64> = REDIS
        .query(|mut q| async move {
            let id: Option<i64> = q.hdel(&hash_key, trigger).await?;
            if let Some(id) = id {
                let key = get_blocklist_key(message, id);
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
                triggers::Column::BlocklistId
                    .eq(id)
                    .and(triggers::Column::Trigger.eq(trigger.as_str())),
            )
            .exec(DB.deref().deref())
            .await?;
    } else {
        let filters = triggers::Entity::find()
            .find_with_related(blocklists::Entity)
            .filter(
                blocklists::Column::Chat
                    .eq(message.get_chat().get_id())
                    .and(triggers::Column::Trigger.eq(trigger.as_str())),
            )
            .all(DB.deref().deref())
            .await?;

        for (f, _) in filters {
            triggers::Entity::delete_many()
                .filter(
                    triggers::Column::Trigger
                        .eq(f.trigger)
                        .and(triggers::Column::BlocklistId.eq(f.blocklist_id)),
                )
                .exec(DB.deref().deref())
                .await?;
        }
    }
    message.speak("Blocklist stopped").await?;
    Ok(())
}

async fn get_blocklist(message: &Message, id: i64) -> Result<Option<blocklists::Model>> {
    default_cache_query(
        |_, _| async move {
            let res = blocklists::Entity::find()
                .filter(blocklists::Column::Id.eq(id))
                .one(DB.deref().deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&get_blocklist_key(message, id), &())
    .await
}

lazy_static! {
    static ref WHITESPACE: Regex = Regex::new(r#"\s+|\S*"#).unwrap();
}

fn iter_whitespace<'a>(text: &'a str) -> Vec<(&'a str, Option<&'a str>)> {
    WHITESPACE
        .find_iter(text)
        .map(|v| v.as_str())
        .chunks(2)
        .into_iter()
        .filter_map(|mut chunks| match (chunks.next(), chunks.next()) {
            (Some(first), Some(next)) => Some((first, Some(next))),
            (Some(first), None) => Some((first, None)),
            _ => None,
        })
        .collect()
}

#[allow(dead_code)]
async fn search_cache_ex(message: &Message, text: &str) -> Result<Option<blocklists::Model>> {
    update_cache_from_db(message).await?;
    let hash_key = get_blocklist_hash_key(message);
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, i64)> = q.hscan(&hash_key).await?;
            while let Some((key, item)) = iter.next_item().await {
                let glob = Glob::new(&key);
                if glob.is_match(text) {
                    return get_blocklist(message, item).await;
                }
            }
            Ok(None)
        })
        .await
}

#[allow(dead_code)]
async fn search_cache(message: &Message, text: &str) -> Result<Option<blocklists::Model>> {
    update_cache_from_db(message).await?;
    let hash_key = get_blocklist_hash_key(message);
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, i64)> = q.hscan(&hash_key).await?;
            while let Some((key, item)) = iter.next_item().await {
                let mut key_iter = iter_whitespace(&key).into_iter().peekable();
                let mut should_match = false;
                if let Some((mut match_word, mut match_ws)) = key_iter.next() {
                    for (word, ws) in iter_whitespace(text) {
                        let matcher = WildMatch::new(&match_word);
                        if matcher.matches(word) {
                            should_match = true;

                            if match_ws != ws {
                                log::info!("NO MATCH whitespace bad");
                                return Ok(None);
                            }

                            if let Some((w, ws)) = key_iter.next() {
                                match_word = w;
                                match_ws = ws;
                            } else {
                                return get_blocklist(message, item).await;
                            }
                        } else if should_match && key_iter.peek().is_some() {
                            log::info!("NO MATCH key empty");
                            return Ok(None);
                        }
                    }
                }
            }
            Ok(None)
        })
        .await
}

async fn update_cache_from_db(message: &Message) -> Result<()> {
    let hash_key = get_blocklist_hash_key(message);
    if !REDIS.sq(|q| q.exists(&hash_key)).await? {
        let res = blocklists::Entity::find()
            .filter(blocklists::Column::Chat.eq(message.get_chat().get_id()))
            .find_with_related(triggers::Entity)
            .all(DB.deref().deref())
            .await?;
        REDIS
            .try_pipe(|p| {
                p.hset(&hash_key, "empty", 0);
                for (filter, triggers) in res.iter() {
                    let key = get_blocklist_key(message, filter.id);
                    let filter_st = RedisStr::new(&filter)?;
                    p.set(&key, filter_st)
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

async fn insert_blocklist(
    message: &Message,
    triggers: &[&str],
    action: ActionType,
    reason: Option<String>,
    duration: Option<Duration>,
) -> Result<()> {
    let model = blocklists::ActiveModel {
        id: ActiveValue::NotSet,
        chat: ActiveValue::Set(message.get_chat().get_id()),
        action: ActiveValue::Set(action),
        reason: ActiveValue::Set(reason),
        duration: ActiveValue::Set(duration.map(|v| v.num_seconds())),
    };

    let model = blocklists::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([
                blocklists::Column::Chat,
                blocklists::Column::Action,
                blocklists::Column::Reason,
            ])
            .update_column(blocklists::Column::Duration)
            .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;
    let triggers = triggers
        .iter()
        .map(|v| v.to_lowercase())
        .collect::<Vec<String>>();
    triggers::Entity::insert_many(
        triggers
            .iter()
            .map(|v| {
                triggers::Model {
                    trigger: (*v).to_owned(),
                    blocklist_id: model.id,
                }
                .into_active_model()
            })
            .collect::<Vec<triggers::ActiveModel>>(),
    )
    .on_conflict(
        OnConflict::columns([triggers::Column::Trigger, triggers::Column::BlocklistId])
            .update_columns([triggers::Column::Trigger, triggers::Column::BlocklistId])
            .to_owned(),
    )
    .exec(DB.deref().deref())
    .await?;
    let hash_key = get_blocklist_hash_key(message);
    let id = model.id.clone();
    REDIS
        .pipe(|p| {
            for trigger in triggers {
                p.hset(&hash_key, trigger, id);
            }
            p
        })
        .await?;
    model.cache(get_blocklist_key(message, id)).await?;
    Ok(())
}

async fn command_blocklist<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;
    let lexer = Lexer::new(args.text);
    let mut parser = Parser::new();
    for token in lexer.all_tokens() {
        parser
            .parse(token)
            .map_err(|e| BotError::speak(e.to_string(), message.get_chat().get_id()))?;
    }

    let cmd = parser
        .end_of_input()
        .map_err(|e| BotError::speak(e.to_string(), message.get_chat().get_id()))?;

    let filters = match cmd.header {
        Header::List(st) => st,
        Header::Arg(st) => vec![st],
    };

    let filters = filters.iter().map(|v| v.as_str()).collect::<Vec<&str>>();
    let (action, duration) = if let Some(v) = cmd.footer {
        let mut args = v.split(" ");
        match args.next() {
            Some("tmute") => (
                ActionType::Mute,
                args.next()
                    .map(|d| parse_duration_str(d, message.get_chat().get_id()).ok())
                    .flatten(),
            ),

            Some("tban") => (
                ActionType::Ban,
                args.next()
                    .map(|d| parse_duration_str(d, message.get_chat().get_id()).ok())
                    .flatten(),
            ),
            Some("twarn") => (
                ActionType::Warn,
                args.next()
                    .map(|d| parse_duration_str(d, message.get_chat().get_id()).ok())
                    .flatten(),
            ),
            None => (ActionType::Mute, None),
            _ => {
                return Err(BotError::speak(
                    "Invalid action",
                    message.get_chat().get_id(),
                ));
            }
        }
    } else {
        (ActionType::Mute, None)
    };
    if let Some(message) = message.get_reply_to_message_ref() {
        insert_blocklist(
            message,
            filters.as_slice(),
            action,
            message.get_text().map(|v| v.into_owned()),
            duration.flatten(),
        )
        .await?;
    } else {
        insert_blocklist(
            message,
            filters.as_slice(),
            action,
            cmd.body,
            duration.flatten(),
        )
        .await?;
    }
    message
        .get_chat()
        .speak(format!("Parsed blocklist item."))
        .await?;
    Ok(())
}

async fn warn(message: &Message, user: &User, reason: Option<String>) -> Result<()> {
    let dialog = dialog_or_default(message.get_chat_ref()).await?;

    let time = dialog.warn_time.map(|t| Duration::seconds(t));
    let count = warn_user(message, user, reason.clone(), &time).await?;

    if count >= dialog.warn_limit {
        match dialog.action_type {
            ActionType::Mute => warn_mute(message, user, count).await,
            ActionType::Ban => warn_ban(message, user, count).await,
            ActionType::Shame => warn_shame(message, user, count).await,
            ActionType::Warn => Ok(()),
        }?;
    }

    let name = user.name_humanreadable();
    if let Some(reason) = reason {
        message
            .reply(format!(
                "Yowzers! Warned user {} for \"{}\", total warns: {}",
                name, reason, count
            ))
            .await?;
    } else {
        message
            .reply(format!(
                "Yowzers! Warned user {}, total warns: {}",
                name, count
            ))
            .await?;
    }
    Ok(())
}

async fn handle_trigger(message: &Message) -> Result<()> {
    if message.get_from().is_admin(message.get_chat_ref()).await? {
        return Ok(());
    }
    if let Some(text) = message.get_text() {
        if let Some(res) = search_cache(message, &text).await? {
            if let Some(user) = message.get_from() {
                let duration = res.duration.map(|v| Duration::seconds(v));
                let duration_str = if let Some(duration) = duration {
                    format!(" for {}", format_duration(duration.to_std()?))
                } else {
                    format!("")
                };
                let reason_str = res
                    .reason
                    .as_ref()
                    .map(|v| format!("Reason: {}", v))
                    .unwrap_or_else(|| format!(""));
                match res.action {
                    ActionType::Mute => {
                        mute(message.get_chat_ref(), &user, duration).await?;
                        message
                            .reply(format!(
                                "User said a banned word. Action: Muted{}\n{}",
                                duration_str, reason_str
                            ))
                            .await?;
                    }
                    ActionType::Ban => {
                        ban(message, &user, duration).await?;
                        message
                            .reply(format!(
                                "User said a banned word. Action: Ban{}\n{}",
                                duration_str, reason_str
                            ))
                            .await?;
                    }
                    ActionType::Warn => {
                        warn(message, &user, res.reason).await?;
                    }
                    ActionType::Shame => (),
                }
            }
        }
    }
    Ok(())
}

async fn list_triggers(message: &Message) -> Result<()> {
    message.group_admin_or_die().await?;
    let hash_key = get_blocklist_hash_key(message);
    update_cache_from_db(message).await?;
    let res: Option<HashMap<String, i64>> = REDIS.sq(|q| q.hgetall(&hash_key)).await?;
    if let Some(map) = res {
        let vals = map
            .into_iter()
            .map(|(key, _)| format!("\t- {}", key))
            .collect_vec()
            .join("\n");
        message
            .reply(format!("Found blocklist items:\n{}", vals))
            .await?;
    } else {
        message.reply("No blocklist items found!").await?;
    }
    Ok(())
}

async fn stopall(message: &Message) -> Result<()> {
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;

    blocklists::Entity::delete_many()
        .filter(blocklists::Column::Chat.eq(message.get_chat().get_id()))
        .exec(DB.deref())
        .await?;

    let key = get_blocklist_hash_key(message);
    REDIS.sq(|q| q.del(&key)).await?;
    message.reply("Stopped all blocklist items").await?;
    Ok(())
}

#[allow(dead_code)]
async fn handle_command<'a>(message: &Message, command: Option<&'a Command<'a>>) -> Result<()> {
    if let Some(&Command { cmd, ref args, .. }) = command {
        match cmd {
            "blocklist" => command_blocklist(message, &args).await?,
            "stopbl" => delete_trigger(message, args.text).await?,
            "blocklists" => list_triggers(message).await?,
            "stopallbl" => stopall(message).await?,
            _ => handle_trigger(message).await?,
        };
    } else {
        handle_trigger(message).await?;
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_update<'a>(update: &UpdateExt, cmd: Option<&'a Command<'a>>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message, cmd).await?,
        _ => (),
    };
    Ok(())
}
