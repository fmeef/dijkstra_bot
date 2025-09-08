use std::collections::HashMap;

use crate::metadata::ModuleHelpers;
use crate::persist::admin::actions::ActionType;
use crate::persist::admin::actions::FilterType;
use crate::persist::redis::default_cache_query;
use crate::persist::redis::CachedQueryTrait;
use crate::persist::redis::RedisCache;
use crate::persist::redis::RedisStr;
use crate::persist::redis::ToRedisStr;
use crate::statics::CONFIG;
use crate::statics::DB;
use crate::statics::REDIS;
use crate::tg::admin_helpers::parse_duration_str;
use crate::tg::admin_helpers::ActionMessage;
use crate::tg::admin_helpers::DeleteAfterTime;
use crate::tg::admin_helpers::UpdateHelpers;
use crate::tg::command::Cmd;
use crate::tg::command::Context;
use crate::tg::command::PopSlice;
use crate::tg::command::TextArgs;
use crate::tg::markdown::Header;
use crate::tg::markdown::MarkupBuilder;
use crate::tg::markdown::MarkupType;
use crate::tg::permissions::*;

use crate::tg::dialog::dialog_or_default;

use crate::tg::user::GetUser;
use crate::util::error::BotError;
use crate::util::error::Fail;
use crate::util::error::Result;

use crate::metadata::metadata;

use crate::util::error::SpeakErr;
use crate::util::glob::WildMatch;

use crate::util::scripting::ModAction;
use crate::util::string::Speak;
use botapi::gen_types::Message;
use botapi::gen_types::User;
use chrono::Duration;
use entities::{blocklists, triggers};
use futures::FutureExt;
use humantime::format_duration;
use itertools::Itertools;
use sea_orm::ModelTrait;

use crate::util::scripting::{ManagedRhai, RHAI_ENGINE};
use lazy_static::lazy_static;
use macros::entity_fmt;
use macros::lang_fmt;
use macros::update_handler;
use redis::AsyncCommands;
use regex::Regex;
use rhai::Dynamic;
use sea_orm::entity::ActiveValue;
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::{NotSet, Set};
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
    { sub = "scripting", content = r#"
    Blocklists now have alpha-quality support for rhai scripting! Scripts allow
    taking arbitrary custom moderation action based on ANY value in telegram's updates.
    Rhai scripts have access to raw telegram types and can inspect anything a standalone bot
    can see! For more information on scripting and rhai syntax please check out
    /help scripting

    [__adding scripts:]\n
    To add a blocklist as a script you can use the /scriptblocklist command.\n
    examples:

    Add a script that checks the message text for "botcoin"
    /scriptblocklist my\_script\_name \|m\| glob\("\*botcoin\*" m.text.value\)

    Add a script that bans any user with username "durov"
    /scriptblocklist no\_pavel \|m\| m.from.value.username == "durov"


    [__blocklists api:]\n
    Each blocklist runs an anonymous function \(for more information see /help scripting\) that
    takes a single parameter containing the incoming message. This function may either
    return a boolean value or a ModAction type. If returning a boolean, true means trigger
    the default blocklist action for that message and false means ignore the message.

    ModAction is a custom type that represents an override action to be taken. Examples are
    [`rust`
    // warns a user with provided reason\n
    ModAction::Warn("reason")\n
    // Warns a user with no reason\n
    ModAction::Warn(())\n
    // Bans a user with provide reason\n
    ModAction::Ban("reason")\n
    // Deletes the message\n
    ModAction::Delete n
    // Replies to the message with provided text\n
    ModAction::Speak("text")\n
    // Mutes the user with the provided reason\n
    ModAction::Mute("reason")
    ]

    Example script that always warns \(even if the default action is not warn\) when a
    premium user speaks\n
    [`rust`
    |m| if m.from.value.is_premium.value {\n
       ModAction::Warn("no premium users allowed")\n
    } else {\n
      ModAction::Ignore\n
    }
    ]
        "#
    },
    { command = "addblocklist", help = "\\<trigger\\> \\<reply\\> {action}: Add a blocklist" },
    { command = "blocklist", help = "List all blocklists" },
    { command = "rmblocklist", help = "Stop a blocklist by trigger" },
    { command = "rmallblocklists", help = "Stop all blocklists" },
    { command = "scriptblocklist", help = "Adds a rhai script as a blocklist with a provided name" },
    { command = "rmscriptblocklist", help = "Moves a script blocklist by name"}
);

struct Migration;
struct MigrationScripting;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230222_000001_create_blocklists"
    }
}

impl MigrationName for MigrationScripting {
    fn name(&self) -> &str {
        "m20240444_000001_create_scripting_blocklist"
    }
}
#[derive(Serialize, Deserialize, Clone)]
enum FilterConfig {
    Text,
    Glob,
    Script(String),
}

impl FilterConfig {
    fn get_type(&self) -> FilterType {
        match self {
            Self::Text => FilterType::Text,
            Self::Script(_) => FilterType::Script,
            Self::Glob => FilterType::Glob,
        }
    }
    fn get_handle(self) -> Option<String> {
        if let FilterConfig::Script(handle) = self {
            Some(handle)
        } else {
            None
        }
    }
}

pub mod entities {
    use crate::persist::{
        admin::actions::{ActionType, FilterType},
        migrate::ManagerHelper,
    };
    use ::sea_orm_migration::prelude::*;
    use chrono::Duration;

    #[async_trait::async_trait]
    impl MigrationTrait for super::MigrationScripting {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(triggers::Entity)
                        .add_column(
                            ColumnDef::new(triggers::Column::FilterType)
                                .integer()
                                .not_null()
                                .default(FilterType::Glob),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(blocklists::Entity)
                        .add_column(
                            ColumnDef::new(blocklists::Column::Handle)
                                .text()
                                .null()
                                .unique_key(),
                        )
                        .to_owned(),
                )
                .await?;
            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(triggers::Entity)
                        .drop_column(triggers::Column::FilterType)
                        .to_owned(),
                )
                .await?;

            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(blocklists::Entity)
                        .drop_column(blocklists::Column::Handle)
                        .to_owned(),
                )
                .await?;
            Ok(())
        }
    }

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
                                .default(Duration::try_minutes(3).unwrap().num_seconds()),
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

        use crate::persist::admin::actions::FilterType;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "blocklist_triggers")]
        pub struct Model {
            #[sea_orm(primary_key, column_type = "Text")]
            pub trigger: String,
            #[sea_orm(primay_key, unique)]
            pub blocklist_id: i64,
            pub filter_type: FilterType,
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
            #[sea_orm(unique)]
            pub handle: Option<String>,
        }

        #[derive(Hash, Eq, PartialEq, Clone, DeriveIntoActiveModel, Debug)]
        pub struct ModelModel {
            pub chat: i64,
            pub action: ActionType,
            pub reason: Option<String>,
            pub duration: Option<i64>,
            pub handle: Option<String>,
        }

        impl From<Model> for ModelModel {
            fn from(value: Model) -> Self {
                ModelModel {
                    chat: value.chat,
                    action: value.action,
                    reason: value.reason,
                    duration: value.duration,
                    handle: value.handle,
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
    vec![Box::new(Migration), Box::new(MigrationScripting)]
}

#[derive(Serialize, Deserialize)]
struct BlockListsExport {
    action: String,
    action_duration: i64,
    default_reason: String,
    filters: Option<Vec<BlocklistFilter>>,
    should_delete: bool,
    handle: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct BlocklistFilter {
    name: String,
    reason: String,
}

lazy_static! {
    static ref FILLER_REGEX: Regex = Regex::new(r#"\{([^}]+)\}"#).unwrap();
}

#[derive(Debug)]
struct Helper;

#[async_trait::async_trait]
impl ModuleHelpers for Helper {
    async fn export(&self, chat: i64) -> Result<Option<serde_json::Value>> {
        let res = blocklists::Entity::find()
            .filter(blocklists::Column::Chat.eq(chat))
            .find_with_related(triggers::Entity)
            .all(*DB)
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
            filters: if items.is_empty() { None } else { Some(items) },
            action_duration: 0,
            default_reason: "".to_owned(),
            should_delete: true,
            action: "nothing".to_owned(),
            handle: None,
        };

        let out = serde_json::to_value(out)?;

        Ok(Some(out))
    }

    async fn import(&self, chat: i64, value: serde_json::Value) -> Result<()> {
        let blocklists: BlockListsExport = serde_json::from_value(value)?;
        if let Some(filters) = blocklists.filters {
            let mut models = HashMap::<blocklists::ModelModel, Vec<String>>::new();

            for blocklist in filters {
                let reason = if blocklist.reason.is_empty() {
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
                    handle: blocklists.handle.clone(),
                };
                let v = models.entry(model).or_default();
                v.push(blocklist.name);
            }

            DB.transaction::<_, _, BotError>(|tx| {
                async move {
                    delete_all(chat).await?;

                    log::info!("import blocklists {:?}", models);
                    blocklists::Entity::insert_many(models.keys().map(|v| {
                        let v = v.clone();
                        blocklists::ActiveModel {
                            id: NotSet,
                            chat: Set(v.chat),
                            action: Set(v.action),
                            reason: Set(v.reason),
                            duration: Set(v.duration),
                            handle: Set(v.handle),
                        }
                    }))
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
                                    filter_type: NotSet,
                                });

                            triggers::Entity::insert_many(trigger).exec(tx).await?;
                        }
                    }
                    Ok(())
                }
                .boxed()
            })
            .await?;
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

async fn delete_script(ctx: &Context, script: String) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members.and(p.can_change_info))
        .await?;
    let hash_key = get_blocklist_hash_key(ctx.message()?.chat.id);

    DB.transaction::<_, (), BotError>(|tx| {
        async move {
            let res = blocklists::Entity::find()
                .find_with_related(triggers::Entity)
                .filter(blocklists::Column::Handle.eq(Some(script)))
                .all(tx)
                .await?;

            for (blocklist, trigger) in res
                .into_iter()
                .map(|(b, t)| (b, t.into_iter().map(|v| v.trigger).collect_vec()))
            {
                let _: () = REDIS.sq(|q| q.hdel(&hash_key, trigger)).await?;
                blocklist.delete(tx).await?;
            }

            Ok(())
        }
        .boxed()
    })
    .await?;

    ctx.reply("Blocklist stopped").await?;

    Ok(())
}

async fn delete_trigger(ctx: &Context, trigger: String) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members.and(p.can_change_info))
        .await?;

    let c = ctx.clone();
    DB.transaction::<_, (), BotError>(move |tx| {
        async move {
            let message = c.message()?;
            let trigger = &trigger.to_lowercase();
            let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
            let filters = blocklists::Entity::find()
                .find_with_related(triggers::Entity)
                .filter(
                    blocklists::Column::Chat
                        .eq(message.get_chat().get_id())
                        .and(triggers::Column::Trigger.eq(trigger.as_str())),
                )
                .all(tx)
                .await?;

            log::info!(
                "deleting {} blocklists for {}",
                filters.len(),
                trigger.as_str()
            );

            for (blocklist, trigger) in filters
                .iter()
                .map(|(b, t)| (b, t.iter().map(|v| v.trigger.as_str()).collect_vec()))
            {
                triggers::Entity::delete_many()
                    .filter(
                        triggers::Column::Trigger
                            .is_in(trigger)
                            .and(triggers::Column::BlocklistId.eq(blocklist.id)),
                    )
                    .exec(tx)
                    .await?;
            }
            REDIS
                .query(|mut q| async move {
                    let id: Option<i64> = q.hdel(&hash_key, trigger).await?;
                    if let Some(id) = id {
                        let key = get_blocklist_key(message, id);
                        let _: () = q.del(&key).await?;
                        Ok(Some(id))
                    } else {
                        Ok(None)
                    }
                })
                .await?;

            Ok(())
        }
        .boxed()
    })
    .await?;
    ctx.reply("Blocklist stopped").await?;

    Ok(())
}

async fn get_blocklist(message: &Message, id: i64) -> Result<Option<blocklists::Model>> {
    default_cache_query(
        |_, _| async move {
            let res = blocklists::Entity::find()
                .filter(blocklists::Column::Id.eq(id))
                .one(*DB)
                .await?;
            Ok(res)
        },
        Duration::try_seconds(CONFIG.timing.cache_timeout).unwrap(),
    )
    .query(&get_blocklist_key(message, id), &())
    .await
}

lazy_static! {
    static ref WHITESPACE: Regex = Regex::new(r#"\s+|\S*"#).unwrap();
}

async fn search_cache(
    ctx: &Context,
    message: &Message,
    text: &str,
) -> Result<Option<blocklists::Model>> {
    update_cache_from_db(message).await?;
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
    REDIS
        .query(|mut q| async move {
            let mut iter: redis::AsyncIter<(String, RedisStr)> = q.hscan(&hash_key).await?;
            while let Some(it) = iter.next_item().await {
                let (key, rs) = it?;
                if key.is_empty() {
                    continue;
                }

                let (item, filtertype): (i64, FilterConfig) = rs.get()?;

                match filtertype {
                    FilterConfig::Glob => {
                        let glob = WildMatch::new(&key);
                        if glob.matches(text) {
                            return get_blocklist(message, item).await;
                        }
                    }
                    FilterConfig::Text => {
                        if text.contains(&key) {
                            return get_blocklist(message, item).await;
                        }
                    }
                    FilterConfig::Script(_) => {
                        let res: Result<Dynamic> = ManagedRhai::new_mapper(
                            key,
                            &RHAI_ENGINE,
                            (message.clone(),),
                        )
                        .post()
                        .await;

                        let res = match res {
                            Ok(action) => {
                                if action.is_bool() {
                                if let Some(res) = action.try_cast::<bool>() {
                                    log::info!("handling bool script {}", res);
                                    if res {
                                        get_blocklist(message, item).await
                                    } else {
                                        Ok(None)
                                    }
                                } else {
                                    Ok(None)
                                }

                                } else {
                                let model = get_blocklist(message, item).await?;
                                let tn = action.type_name();
                                let res = match (action.try_cast::<ModAction>(), model) {
                                    (Some(ModAction::Reply(reply)), _) => {
                                        ctx.reply(reply).await?;
                                        None
                                    }
                                    (Some(ModAction::Ignore), _) => None,
                                    (Some(modaction), Some(mut model)) => {
                                        if let Some(action) = modaction.get_action_type() {
                                            model.action = action;
                                        }
                                        model.reason = modaction.to_reason();
                                        Some(model)
                                    }
                                    (None, Some(mut model)) => {
                                        model.action = ActionType::Delete;
                                        model.reason = None;
                                        ctx.reply(format!("Blocklist mapper function returned invalid type. Was {}, expected bool or ModAction", tn)).await?;
                                        Some(model)
                                    }
                                    (_, None) => None,
                                };

                                Ok(res)

                                }
                            }
                            Err(err) => {
                                let mut bl = get_blocklist(message, item).await?;
                                if let Some(bl) = bl.as_mut() {
                                    bl.action = ActionType::Delete;
                                    bl.reason = None;
                                    ctx.reply(format!(
                                        "Failed to block message, rhai error: {}",
                                        err
                                    ))
                                    .await?;
                                }
                                Ok(bl)
                            }

                        };
                    if let Ok(res) = res {
                        if res.is_some() {
                            return Ok(res);
                        }
                    }

                    }
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
            .all(*DB)
            .await?;
        let _: () = REDIS
            .try_pipe(|p| {
                p.hset(&hash_key, "", 0);
                for (filter, triggers) in res.into_iter() {
                    let key = get_blocklist_key(message, filter.id);
                    let filter_st = RedisStr::new(&filter)?;
                    p.set(&key, filter_st)
                        .expire(&key, CONFIG.timing.cache_timeout);
                    for trigger in triggers.into_iter() {
                        p.hset(
                            &hash_key,
                            trigger.trigger,
                            (filter.id, Some(filter.handle.as_ref())).to_redis()?,
                        )
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
    filter_type: FilterConfig,
) -> Result<()> {
    let ft = filter_type.get_type();
    let triggers = triggers
        .iter()
        .map(|v| {
            if let FilterConfig::Script(_) = filter_type {
                (*v).to_owned()
            } else {
                v.to_lowercase()
            }
        })
        .collect::<Vec<String>>();

    let model = blocklists::ActiveModel {
        id: ActiveValue::NotSet,
        chat: ActiveValue::Set(message.get_chat().get_id()),
        action: ActiveValue::Set(action),
        reason: ActiveValue::Set(reason),
        duration: ActiveValue::Set(duration.map(|v| v.num_seconds())),
        handle: ActiveValue::Set(filter_type.clone().get_handle()),
    };

    let model = blocklists::Entity::insert(model);

    let model = if let FilterConfig::Script(_) = filter_type {
        model.on_conflict(
            OnConflict::column(blocklists::Column::Handle)
                .update_column(blocklists::Column::Duration)
                .to_owned(),
        )
    } else {
        model.on_conflict(
            OnConflict::columns([
                blocklists::Column::Chat,
                blocklists::Column::Action,
                blocklists::Column::Reason,
            ])
            .update_column(blocklists::Column::Duration)
            .to_owned(),
        )
    }
    .exec_with_returning(*DB)
    .await?;

    let t = triggers
        .iter()
        .map(|v| {
            triggers::Model {
                trigger: (*v).to_owned(),
                blocklist_id: model.id,
                filter_type: ft,
            }
            .into_active_model()
        })
        .collect::<Vec<triggers::ActiveModel>>();

    triggers::Entity::insert_many(t)
        .on_conflict(
            OnConflict::columns([triggers::Column::Trigger, triggers::Column::BlocklistId])
                .update_columns([triggers::Column::Trigger, triggers::Column::BlocklistId])
                .to_owned(),
        )
        .exec(*DB)
        .await?;
    let hash_key = get_blocklist_hash_key(message.get_chat().get_id());
    let id = (model.id, filter_type).to_redis()?;
    let model_id = model.id;
    let _: () = REDIS
        .pipe(|p| {
            for trigger in triggers {
                p.hset(&hash_key, trigger, &id);
            }
            p
        })
        .await?;
    model.cache(get_blocklist_key(message, model_id)).await?;
    Ok(())
}

async fn command_blocklist<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    log::info!("adding blocklist ");
    let message = ctx.message()?;

    let cmd = MarkupBuilder::new(None)
        .set_text(args.text.to_owned())
        .filling(false)
        .show_fillings(false)
        .header(true);

    let (body, _, _, header, footer) = cmd.build_filter().await;
    let filters = match header.ok_or_else(|| ctx.fail_err("Header missing from filter command"))? {
        Header::List(st) => st,
        Header::Arg(st) => vec![st],
    };

    let filters = filters.iter().map(|v| v.as_str()).collect::<Vec<&str>>();
    let (action, duration) = if let Some(v) = footer.last() {
        let mut args = v.split(' ');
        match args.next() {
            Some("tmute") => (
                ActionType::Mute,
                args.next().and_then(|d| {
                    parse_duration_str(d, message.get_chat().get_id(), message.message_id).ok()
                }),
            ),

            Some("tban") => (
                ActionType::Ban,
                args.next().and_then(|d| {
                    parse_duration_str(d, message.get_chat().get_id(), message.message_id).ok()
                }),
            ),
            Some("twarn") => (
                ActionType::Warn,
                args.next().and_then(|d| {
                    parse_duration_str(d, message.get_chat().get_id(), message.message_id).ok()
                }),
            ),
            None => (ActionType::Delete, None),
            _ => {
                return Err(BotError::speak(
                    "Invalid action",
                    message.get_chat().get_id(),
                    Some(message.message_id),
                ));
            }
        }
    } else {
        (ActionType::Delete, None)
    };

    let (f, message) = if let Some(message) = message.get_reply_to_message() {
        (message.get_text().map(|v| v.to_owned()), message)
    } else {
        (Some(body), message)
    };
    insert_blocklist(
        message,
        filters.as_slice(),
        action,
        f,
        duration.flatten(),
        FilterConfig::Glob,
    )
    .await?;

    let filters = [""]
        .into_iter()
        .chain(filters.into_iter())
        .collect::<Vec<&str>>()
        .join("\n - ");
    //  let filters = format!("\n{}", filters);

    let text = MarkupType::Code.text(&filters);

    message
        .get_chat()
        .reply_fmt(entity_fmt!(ctx, "addblocklist", text))
        .await?;

    Ok(())
}

async fn script_blocklist<'a>(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    log::info!("adding script blocklist ");
    ctx.action_message(|ctx, am, args| async move {
        let (name, args) = args
            .and_then(|a| a.pop_slice())
            .ok_or_else(|| ctx.fail_err("Need to provide a script name"))?;
        let (message, text) = match am {
            ActionMessage::Me(message) => (message, args.text),
            ActionMessage::Reply(message) => (
                message,
                message
                    .text
                    .as_deref()
                    .ok_or_else(|| ctx.fail_err("The replied message has no text"))?,
            ),
        };

        if text.trim().is_empty() {
            let res = blocklists::Entity::find()
                .find_with_related(triggers::Entity)
                .filter(blocklists::Column::Handle.eq(name.get_text()))
                .all(*DB)
                .await?;
            if let Some((_, triggers)) = res.first() {
                if let Some(trigger) = triggers.first() {
                    let t = MarkupType::Pre(Some("rust".to_owned())).text(&trigger.trigger);
                    ctx.reply_fmt(entity_fmt!(ctx, "empty", t)).await?;
                    return Ok(());
                }
            }
            ctx.reply(format!(
                "The script blocklist {} does not exist",
                name.get_text()
            ))
            .await?;
        } else {
            ManagedRhai::new_mapper(text.to_owned(), &RHAI_ENGINE, (message.clone(),))
                .compile()
                .speak_err(ctx, |e| {
                    format!("Failed to compile blocklist script: {}", e)
                })
                .await?;

            insert_blocklist(
                message,
                &[text],
                ActionType::Delete,
                None,
                None,
                FilterConfig::Script(name.get_text().to_owned()),
            )
            .await?;
            let text = MarkupType::Pre(Some("rust".to_owned())).text(text);

            message
                .get_chat()
                .reply_fmt(entity_fmt!(ctx, "addscriptlocklist", text))
                .await?;
        }
        Ok(())
    })
    .await?;

    //  let filters = format!("\n{}", filters);

    Ok(())
}

async fn warn(ctx: &Context, user: &User, reason: Option<String>) -> Result<()> {
    let dialog = dialog_or_default(ctx.message()?.get_chat()).await?;

    let time = dialog.warn_time.and_then(Duration::try_seconds);
    ctx.warn_with_action(user.get_id(), reason.clone().as_deref(), time)
        .await?;
    Ok(())
}

async fn handle_trigger(ctx: &Context) -> Result<()> {
    if let Some(message) = ctx.should_moderate().await {
        if let Some(user) = message.get_from() {
            if let Some(text) = message.get_text() {
                if let Some(res) = search_cache(ctx, message, text).await? {
                    let duration = res.duration.and_then(Duration::try_seconds);
                    let duration_str = if let Some(duration) = duration {
                        lang_fmt!(ctx, "duration", format_duration(duration.to_std()?))
                    } else {
                        String::new()
                    };
                    let reason_str = res
                        .reason
                        .as_ref()
                        .map(|v| lang_fmt!(ctx, "reason", v))
                        .unwrap_or_default();
                    match res.action {
                        ActionType::Mute => {
                            ctx.mute(user.get_id(), ctx.try_get()?.chat, duration)
                                .await?;
                            let mention = user.mention().await?;
                            message
                                .reply_fmt(entity_fmt!(
                                    ctx,
                                    "blockmute",
                                    mention,
                                    duration_str,
                                    reason_str
                                ))
                                .await?;
                        }
                        ActionType::Ban => {
                            ctx.ban(user.get_id(), duration, true).await?;
                            let mention = user.mention().await?;
                            message
                                .reply_fmt(entity_fmt!(
                                    ctx,
                                    "blockban",
                                    mention,
                                    duration_str,
                                    reason_str
                                ))
                                .await?;
                        }
                        ActionType::Warn => {
                            warn(ctx, user, res.reason).await?;
                        }
                        ActionType::Shame => (),
                        ActionType::Delete => (),
                    }
                    message.delete().await?;
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
    let res: Option<HashMap<String, RedisStr>> = REDIS.sq(|q| q.hgetall(&hash_key)).await?;
    if let Some(map) = res {
        let iter = map
            .iter()
            .filter_map(|(k, v)| {
                let v: Option<(i64, FilterConfig)> = v.get().ok();
                v.map(|v| (k, v))
            })
            .filter(|(k, _)| !k.is_empty());

        let scripts = iter
            .clone()
            .filter_map(|(_, v)| {
                if let FilterConfig::Script(handle) = v.1 {
                    Some(format!("\t- {}", handle))
                } else {
                    None
                }
            })
            .join("\n");

        let vals = iter
            .filter_map(|(n, v)| {
                if let FilterConfig::Glob = v.1 {
                    Some(format!("\t- {}", n))
                } else {
                    None
                }
            })
            .join("\n");

        message
            .reply(format!(
                "Found text blocklists:\n{}\n\nFound script blocklists:\n{}",
                vals, scripts
            ))
            .await?;
    } else {
        message.reply("No blocklist items found!").await?;
    }
    Ok(())
}

async fn delete_all(chat: i64) -> Result<()> {
    blocklists::Entity::delete_many()
        .filter(blocklists::Column::Chat.eq(chat))
        .exec(*DB)
        .await?;

    let key = get_blocklist_hash_key(chat);
    REDIS.sq(|q| q.del(&key)).await
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
            "addblocklist" => command_blocklist(ctx, args).await?,
            "scriptblocklist" => script_blocklist(ctx).await?,
            "rmblocklist" => delete_trigger(ctx, args.text.to_owned()).await?,
            "rmscriptblocklist" => delete_script(ctx, args.text.to_owned()).await?,
            "blocklist" => list_triggers(message).await?,
            "rmallblocklists" => stopall(ctx, ctx.message()?.get_chat().get_id()).await?,
            _ => handle_trigger(ctx).await?,
        };
    }

    handle_trigger(ctx).await?;

    Ok(())
}

#[update_handler]
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
