use std::collections::HashMap;

use crate::metadata::ModuleHelpers;
use crate::persist::admin::actions::ActionType;
use crate::persist::redis::default_cache_query;
use crate::persist::redis::CachedQueryTrait;
use crate::persist::redis::RedisCache;
use crate::persist::redis::RedisStr;
use crate::statics::CONFIG;
use crate::statics::DB;
use crate::statics::REDIS;
use crate::statics::TG;
use crate::tg::admin_helpers::is_approved;
use crate::tg::admin_helpers::is_dm;
use crate::tg::admin_helpers::parse_duration_str;
use crate::tg::command::Cmd;
use crate::tg::command::Context;
use crate::tg::command::TextArgs;
use crate::tg::markdown::Header;
use crate::tg::markdown::MarkupBuilder;
use crate::tg::markdown::MarkupType;
use crate::tg::permissions::*;

use crate::tg::dialog::dialog_or_default;

use crate::util::error::BotError;
use crate::util::error::Fail;
use crate::util::error::Result;

use crate::metadata::metadata;

use crate::util::glob::WildMatch;

use crate::util::string::Speak;
use botapi::gen_types::Message;
use botapi::gen_types::User;
use chrono::Duration;
use entities::{blocklists, triggers};
use futures::FutureExt;
use humantime::format_duration;
use itertools::Itertools;
use lazy_static::__Deref;
use lazy_static::lazy_static;
use macros::entity_fmt;
use redis::AsyncCommands;
use regex::Regex;
use sea_orm::entity::ActiveValue;
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::Set;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::IntoActiveModel;
use sea_orm::QueryFilter;
use sea_orm::TransactionTrait;
use sea_orm_migration::{MigrationName, MigrationTrait};
use serde::{Deserialize, Serialize};

metadata!("Blocklists",
    r#"Censor specific words in your group!. Supports globbing to match partial words."#,
    Helper,
    { command = "addblocklist", help = "\\<trigger\\> \\<reply\\> {action}: Add a blocklist" },
    { command = "blocklist", help = "List all blocklists" },
    { command = "rmblocklist", help = "Stop a blocklist by trigger" },
    { command = "rmallblocklists", help = "Stop all blocklists" }
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
                                .default(ActionType::Delete),
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

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, Hash)]
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

        #[derive(DeriveIntoActiveModel, Hash, Eq, PartialEq, Clone)]
        pub struct ModelModel {
            #[sea_orm(primary_key)]
            pub chat: i64,
            pub action: ActionType,
            pub reason: Option<String>,
            pub duration: Option<i64>,
        }

        impl Into<ModelModel> for Model {
            fn into(self) -> ModelModel {
                ModelModel {
                    chat: self.chat,
                    action: self.action,
                    reason: self.reason,
                    duration: self.duration,
                }
            }
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::triggers::Entity",
                from = "Column::Id",
                to = "super::triggers::Column::BlocklistId"
            )]
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

#[derive(Serialize, Deserialize)]
struct BlockListsExport {
    action: String,
    action_duration: i64,
    default_reason: String,
    filters: Option<Vec<BlocklistFilter>>,
    should_delete: bool,
}

#[derive(Serialize, Deserialize)]
struct BlocklistFilter {
    name: String,
    reason: String,
}

lazy_static! {
    static ref FILLER_REGEX: Regex = Regex::new(r#"\{([^}]+)\}"#).unwrap();
}

struct Helper;

#[async_trait::async_trait]
impl ModuleHelpers for Helper {
    async fn export(&self, chat: i64) -> Result<Option<serde_json::Value>> {
        let res = blocklists::Entity::find()
            .filter(blocklists::Column::Chat.eq(chat))
            .find_with_related(triggers::Entity)
            .all(DB.deref())
            .await?;

        let items: Vec<BlocklistFilter> = res
            .into_iter()
            .flat_map(|(blocklist, trigger)| {
                trigger.into_iter().map(move |trigger| {
                    let action = match blocklist.action {
                        ActionType::Mute => {
                            if let Some(duration) = blocklist.duration {
                                format!("{{tmute {}}}", duration)
                            } else {
                                "{mute}".to_owned()
                            }
                        }
                        ActionType::Ban => "{ban}".to_owned(),
                        ActionType::Shame => "".to_owned(),
                        ActionType::Warn => "".to_owned(),
                        ActionType::Delete => "{del}".to_owned(),
                    };
                    BlocklistFilter {
                        name: trigger.trigger,
                        reason: blocklist
                            .reason
                            .clone()
                            .map(|reason| format!("{} {}", reason, action))
                            .unwrap_or_else(|| "".to_owned()),
                    }
                })
            })
            .collect();
        let out = BlockListsExport {
            filters: if items.len() == 0 { None } else { Some(items) },
            action_duration: 0,
            default_reason: "".to_owned(),
            should_delete: true,
            action: "nothing".to_owned(),
        };

        let out = serde_json::to_value(&out)?;

        Ok(Some(out))
    }

    async fn import(&self, chat: i64, value: serde_json::Value) -> Result<()> {
        let blocklists: BlockListsExport = serde_json::from_value(value)?;
        if let Some(filters) = blocklists.filters {
            let mut models = HashMap::<blocklists::ModelModel, Vec<String>>::new();

            for blocklist in filters {
                let reason = if blocklist.reason.len() == 0 {
                    None
                } else {
                    Some(blocklist.reason)
                };
                let mut action = ActionType::Delete;
                let mut duration = None;
                if let Some(reason) = &reason {
                    for filler in FILLER_REGEX.find_iter(reason) {
                        let mut filler = filler.as_str().split_whitespace();
                        let (a, d) = match filler.next() {
                            Some("mute") => (ActionType::Mute, None),
                            _ => continue,
                        };

                        action = a;
                        duration = d;
                    }
                }
                let model = blocklists::ModelModel {
                    chat,
                    action,
                    reason,
                    duration,
                };
                let v = models.entry(model).or_insert_with(|| Vec::new());
                v.push(blocklist.name);
            }

            DB.transaction::<_, _, BotError>(|tx| {
                async move {
                    blocklists::Entity::insert_many(
                        models.keys().map(|v| v.clone().into_active_model()),
                    )
                    .on_empty_do_nothing()
                    .exec(tx)
                    .await?;

                    let res = blocklists::Entity::find()
                        .filter(blocklists::Column::Chat.eq(chat))
                        .all(tx)
                        .await?;

                    for model in res {
                        let id = model.id;
                        let modelmodel: blocklists::ModelModel = model.into();

                        if let Some(trigger) = models.remove(&modelmodel) {
                            let trigger =
                                trigger.into_iter().map(|trigger| triggers::ActiveModel {
                                    blocklist_id: Set(id),
                                    trigger: Set(trigger),
                                });

                            triggers::Entity::insert_many(trigger).exec(tx).await?;
                        }
                    }
                    Ok(())
                }
                .boxed()
            })
            .await?;

            delete_all(chat).await?;
        }
        Ok(())
    }

    fn supports_export(&self) -> Option<&'static str> {
        Some("blocklists")
    }

    fn get_migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        get_migrations()
    }
}

fn get_blocklist_key(message: &Message, id: i64) -> String {
    format!("blockl:{}:{}", message.get_chat().get_id(), id)
}

fn get_blocklist_hash_key(chat: i64) -> String {
    format!("bcache:{}", chat)
}

async fn delete_trigger(message: &Message, trigger: &str) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members.and(p.can_change_info))
        .await?;
    let trigger = &trigger.to_lowercase();
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
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
            .exec(DB.deref())
            .await?;
    } else {
        let filters = triggers::Entity::find()
            .find_with_related(blocklists::Entity)
            .filter(
                blocklists::Column::Chat
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
                        .and(triggers::Column::BlocklistId.eq(f.blocklist_id)),
                )
                .exec(DB.deref())
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
                .one(DB.deref())
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

#[allow(dead_code)]
async fn search_cache(message: &Message, text: &str) -> Result<Option<blocklists::Model>> {
    update_cache_from_db(message).await?;
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, i64)> = q.hscan(&hash_key).await?;
            while let Some((key, item)) = iter.next_item().await {
                let glob = WildMatch::new(&key);
                if glob.matches(text) {
                    return get_blocklist(message, item).await;
                }
            }
            Ok(None)
        })
        .await
}

async fn update_cache_from_db(message: &Message) -> Result<()> {
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
    let k: usize = REDIS.sq(|q| q.exists(&hash_key)).await?;
    if k == 0 {
        let res = blocklists::Entity::find()
            .filter(blocklists::Column::Chat.eq(message.get_chat().get_id()))
            .find_with_related(triggers::Entity)
            .all(DB.deref())
            .await?;
        REDIS
            .try_pipe(|p| {
                p.hset(&hash_key, "empty", 0);
                for (filter, triggers) in res.into_iter() {
                    let key = get_blocklist_key(message, filter.id);
                    let filter_st = RedisStr::new(&filter)?;
                    p.set(&key, filter_st)
                        .expire(&key, CONFIG.timing.cache_timeout);
                    for trigger in triggers.into_iter() {
                        p.hset(&hash_key, trigger.trigger, filter.id)
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
        .exec_with_returning(DB.deref())
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
    .exec(DB.deref())
    .await?;
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
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

async fn command_blocklist<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;

    let message = ctx.message()?;

    let cmd = MarkupBuilder::new(None)
        .set_text(args.text.to_owned())
        .filling(false)
        .header(true);

    let (body, _, _, header, footer) = cmd.build_filter().await;
    let filters = match header.ok_or_else(|| ctx.fail_err("Header missing from filter command"))? {
        Header::List(st) => st,
        Header::Arg(st) => vec![st],
    };

    let filters = filters.iter().map(|v| v.as_str()).collect::<Vec<&str>>();
    let (action, duration) = if let Some(v) = footer {
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
            None => (ActionType::Delete, None),
            _ => {
                return Err(BotError::speak(
                    "Invalid action",
                    message.get_chat().get_id(),
                ));
            }
        }
    } else {
        (ActionType::Delete, None)
    };

    let (f, message) = if let Some(message) = message.get_reply_to_message_ref() {
        (message.get_text().map(|v| v.into_owned()), message)
    } else {
        (Some(body), message)
    };
    insert_blocklist(message, filters.as_slice(), action, f, duration.flatten()).await?;

    let filters = [""]
        .into_iter()
        .chain(filters.into_iter())
        .collect::<Vec<&str>>()
        .join("\n - ");
    //  let filters = format!("\n{}", filters);

    let text = MarkupType::Code.text(&filters);

    message
        .get_chat()
        .speak_fmt(entity_fmt!(ctx, "addblocklist", text))
        .await?;

    Ok(())
}

async fn delete(message: &Message) -> Result<()> {
    TG.client
        .build_delete_message(message.get_chat().get_id(), message.get_message_id())
        .build()
        .await?;
    Ok(())
}

async fn warn(ctx: &Context, user: &User, reason: Option<String>) -> Result<()> {
    let dialog = dialog_or_default(ctx.message()?.get_chat_ref()).await?;

    let time = dialog.warn_time.map(|t| Duration::seconds(t));
    ctx.warn_with_action(
        user.get_id(),
        reason.clone().as_ref().map(|v| v.as_str()),
        time,
    )
    .await?;
    Ok(())
}

async fn handle_trigger(ctx: &Context) -> Result<()> {
    if let Ok(message) = ctx.message() {
        if let Some(user) = message.get_from() {
            if message.get_from().is_admin(message.get_chat_ref()).await?
                || is_dm(message.get_chat_ref())
                || is_approved(message.get_chat_ref(), &user).await?
            {
                log::info!(
                    "skipping trigger {}",
                    message.get_from().is_admin(message.get_chat_ref()).await?
                );
                return Ok(());
            }

            if let Some(text) = message.get_text() {
                if let Some(res) = search_cache(message, &text).await? {
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
                            ctx.mute(user.get_id(), ctx.try_get()?.chat, duration)
                                .await?;
                            message
                                .reply(format!(
                                    "User said a banned word. Action: Muted{}\n{}",
                                    duration_str, reason_str
                                ))
                                .await?;
                        }
                        ActionType::Ban => {
                            ctx.ban(user.get_id(), duration).await?;
                            message
                                .reply(format!(
                                    "User said a banned word. Action: Ban{}\n{}",
                                    duration_str, reason_str
                                ))
                                .await?;
                        }
                        ActionType::Warn => {
                            warn(ctx, &user, res.reason).await?;
                        }
                        ActionType::Shame => (),
                        ActionType::Delete => (),
                    }
                    delete(message).await?;
                }
            }
        }
    }
    Ok(())
}

async fn list_triggers(message: &Message) -> Result<()> {
    message.check_permissions(|p| p.can_manage_chat).await?;
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
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

async fn delete_all(chat: i64) -> Result<()> {
    blocklists::Entity::delete_many()
        .filter(blocklists::Column::Chat.eq(chat))
        .exec(DB.deref())
        .await?;

    let key = get_blocklist_hash_key(chat);
    REDIS.sq(|q| q.del(&key)).await?;
    Ok(())
}

async fn stopall(ctx: &Context, chat: i64) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    delete_all(chat).await?;
    ctx.reply("Stopped all blocklist items").await?;
    Ok(())
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
            "addblocklist" => command_blocklist(ctx, &args).await?,
            "rmblocklist" => delete_trigger(message, args.text).await?,
            "blocklist" => list_triggers(message).await?,
            "rmallblocklists" => stopall(ctx, ctx.message()?.get_chat().get_id()).await?,
            _ => handle_trigger(ctx).await?,
        };
    }

    handle_trigger(&ctx).await?;

    Ok(())
}

pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}

#[allow(unused_imports)]
mod test {
    use super::FILLER_REGEX;

    #[test]
    fn regex_match() {
        let mut m = FILLER_REGEX.find_iter("{filler} {filler2}");
        assert_eq!(m.next().map(|m| m.as_str()), Some("{filler}"));
        assert_eq!(m.next().map(|m| m.as_str()), Some("{filler2}"));
    }
}
