use self::entities::{default_locks, locks};
use crate::persist::admin::actions::ActionType;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::{CONFIG, DB, REDIS};
use crate::tg::admin_helpers::{mute, self_admin_or_die, IsAdmin};
use crate::tg::command::{Command, Context, TextArg, TextArgs};
use crate::tg::user::Username;
use crate::util::error::{BotError, Result};
use crate::util::string::get_chat_lang;
use crate::{metadata::metadata, statics::TG, util::string::Speak};
use botapi::gen_types::{Chat, Message, UpdateExt, User};
use chrono::Duration;
use entities::locks::LockType;
use lazy_static::__Deref;
use macros::lang_fmt;
use redis::AsyncCommands;
use sea_orm::prelude::*;
use sea_orm::sea_query::OnConflict;

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::EntityTrait;
use sea_orm_migration::{MigrationName, MigrationTrait};

metadata!("Locks",
    r#"
    Are blue star check mark users ruining your group with their endless pop-psychobabble and
    coin scams? Lock the group to keep the premiums out.
    "#,
    { command = "lock", help = "Engage a lock" },
    { command = "unlock", help = "Disable a lock"},
    { command = "locks", help = "Get a list of active locks"}
);

pub mod entities {
    use self::locks::LockAction;
    use crate::persist::admin::actions::ActionType;
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;

    use super::Migration;
    use super::MigrationActionType;

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(locks::Entity)
                        .col(ColumnDef::new(locks::Column::Chat).big_integer().not_null())
                        .col(ColumnDef::new(locks::Column::LockType).integer().not_null())
                        .col(
                            ColumnDef::new(locks::Column::LockAction)
                                .integer()
                                .not_null()
                                .default(LockAction::Silent),
                        )
                        .primary_key(
                            IndexCreateStatement::new()
                                .col(locks::Column::Chat)
                                .col(locks::Column::LockType)
                                .primary(),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager.drop_table_auto(locks::Entity).await?;
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for MigrationActionType {
        async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(locks::Entity)
                        .modify_column(ColumnDef::new(locks::Column::LockAction).integer())
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(default_locks::Entity)
                        .col(
                            ColumnDef::new(locks::Column::Chat)
                                .big_integer()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(locks::Column::LockAction)
                                .integer()
                                .not_null()
                                .default(ActionType::Delete),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(locks::Entity)
                        .modify_column(
                            ColumnDef::new(locks::Column::LockAction)
                                .integer()
                                .not_null()
                                .default(LockAction::Silent),
                        )
                        .to_owned(),
                )
                .await?;
            manager.drop_table_auto(default_locks::Entity).await?;
            Ok(())
        }
    }

    pub mod default_locks {

        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        use crate::persist::admin::actions::ActionType;

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::locks::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "default_locks")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(default = ActionType::Delete)]
            pub lock_action: ActionType,
        }
    }

    pub mod locks {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        use crate::persist::admin::actions::ActionType;

        #[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug)]
        #[sea_orm(rs_type = "i32", db_type = "Integer")]
        pub enum LockType {
            #[sea_orm(num_value = 1)]
            Premium,
        }

        impl LockType {
            pub fn get_name(&self) -> String {
                match self {
                    Self::Premium => "Premium mambers".to_owned(),
                }
            }
        }

        #[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug)]
        #[sea_orm(rs_type = "i32", db_type = "Integer")]
        pub enum LockAction {
            #[sea_orm(num_value = 1)]
            Mute,
            #[sea_orm(num_value = 2)]
            Warn,
            #[sea_orm(num_value = 3)]
            Silent,
        }

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "locks")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(primary_key)]
            pub lock_type: LockType,
            #[sea_orm(default = ActionType::Delete)]
            pub lock_action: Option<ActionType>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::locks::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

pub struct Migration;
pub struct MigrationActionType;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230129_000001_create_locks"
    }
}

impl MigrationName for MigrationActionType {
    fn name(&self) -> &str {
        "m20230316_000001_update_action_type"
    }
}

pub fn get_lock_key(chat: i64, locktype: &LockType) -> String {
    format!("lock:{}:{}", chat, locktype.to_string())
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration), Box::new(MigrationActionType)]
}

async fn get_lock(message: &Message, locktype: LockType) -> Result<Option<locks::Model>> {
    let chat = message.get_chat().get_id();
    let key = get_lock_key(chat, &locktype);
    default_cache_query(
        |_, _| async move {
            let res = locks::Entity::find_by_id((chat, locktype))
                .one(DB.deref().deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await
}

async fn clear_lock(message: &Message, locktype: LockType) -> Result<()> {
    let chat = message.get_chat().get_id();
    let key = get_lock_key(chat, &locktype);
    locks::Entity::delete_by_id((chat, locktype))
        .exec(DB.deref().deref())
        .await?;
    REDIS.sq(|q| q.del(&key)).await?;
    Ok(())
}

async fn set_lock(message: &Message, locktype: LockType, user: &User) -> Result<()> {
    user.admin_or_die(message.get_chat_ref()).await?;
    let key = get_lock_key(message.get_chat().get_id(), &locktype);
    let model = locks::ActiveModel {
        chat: Set(message.get_chat().get_id()),
        lock_type: Set(locktype),
        lock_action: NotSet,
    };
    let res = locks::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([locks::Column::Chat, locks::Column::LockType])
                .update_column(locks::Column::LockAction)
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;
    res.cache(key).await?;
    Ok(())
}

#[inline(always)]
fn get_default_key(chat: &Chat) -> String {
    format!("daction:{}", chat.get_id())
}

async fn get_default_action(chat: &Chat) -> Result<ActionType> {
    let chat_id = chat.get_id();
    let key = get_default_key(chat);
    default_cache_query(
        |_, _| async move {
            let model = default_locks::Entity::find_by_id(chat_id)
                .one(DB.deref())
                .await?;
            Ok(model.map(|v| v.lock_action).unwrap_or(ActionType::Delete))
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await
}

async fn set_default_action(chat: &Chat, lock_action: ActionType) -> Result<()> {
    let model = default_locks::Model {
        chat: chat.get_id(),
        lock_action,
    };
    let key = get_default_key(chat);
    default_locks::Entity::insert(model.cache(&key).await?)
        .on_conflict(
            OnConflict::column(default_locks::Column::Chat)
                .update_column(default_locks::Column::LockAction)
                .to_owned(),
        )
        .exec(DB.deref())
        .await?;
    Ok(())
}

async fn set_lock_action(
    message: &Message,
    locktype: LockType,
    lockaction: ActionType,
    user: &User,
) -> Result<()> {
    user.admin_or_die(message.get_chat_ref()).await?;
    let key = get_lock_key(message.get_chat().get_id(), &locktype);
    let model = locks::Model {
        chat: message.get_chat().get_id(),
        lock_type: locktype,
        lock_action: Some(lockaction),
    };
    locks::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::columns([locks::Column::Chat, locks::Column::LockType])
                .update_column(locks::Column::LockAction)
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;
    Ok(())
}

fn locktype_from_args<'a>(
    cmd: &Option<&'a Command<'a>>,
    chat: i64,
) -> (Option<LockType>, Option<ActionType>) {
    if let Some(&Command { ref args, .. }) = cmd {
        match args.args.first() {
            Some(TextArg::Arg("premium")) => (
                Some(LockType::Premium),
                args.args
                    .get(1)
                    .map(|v| ActionType::from_str(v.get_text(), chat).ok())
                    .flatten(),
            ),
            _ => (None, None),
        }
    } else {
        (None, None)
    }
}

fn is_premium(message: &Message) -> bool {
    if let Some(user) = message.get_from() {
        user.get_is_premium().unwrap_or(false)
    } else {
        false
    }
}

async fn handle_lock<'a>(message: &Message, cmd: &Option<&Command<'a>>, user: &User) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id());
    user.admin_or_die(message.get_chat_ref()).await?;
    match locktype_from_args(cmd, message.get_chat().get_id()) {
        (Some(lock), None) => {
            let t = lock.get_name();
            set_lock(message, lock, user).await?;

            message
                .reply(lang_fmt!(
                    lang,
                    "setlock",
                    t,
                    message.get_chat().name_humanreadable()
                ))
                .await?;
        }
        (Some(lock), Some(action)) => {
            let reply = lang_fmt!(lang, "setlockaction", action.get_name());
            set_lock_action(message, lock, action, user).await?;
            message.reply(reply).await?;
        }
        _ => {
            message.reply(lang_fmt!(lang, "locknotspec")).await?;
        }
    };
    Ok(())
}

async fn handle_unlock<'a>(
    message: &Message,
    cmd: &Option<&Command<'a>>,
    user: &User,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id());
    user.admin_or_die(message.get_chat_ref()).await?;
    if let (Some(lock), _) = locktype_from_args(cmd, message.get_chat().get_id()) {
        let name = lock.get_name();
        clear_lock(message, lock).await?;
        message.reply(lang_fmt!(lang, "clearedlock", name)).await?;
    } else {
        message.reply(lang_fmt!(lang, "locknotspec")).await?;
    }
    Ok(())
}

async fn handle_list(message: &Message, user: &User) -> Result<()> {
    user.admin_or_die(message.get_chat_ref()).await?;
    let chat = message.get_chat().get_id();
    let locks = locks::Entity::find()
        .filter(locks::Column::Chat.eq(chat))
        .all(DB.deref().deref())
        .await?;

    if locks.len() > 0 {
        let print = locks
            .iter()
            .map(|v| format!("\t-{}", v.lock_type.get_name()))
            .collect::<Vec<String>>()
            .join("\n");
        message.reply(format!("Enabled locks: \n{}", print)).await?;
    } else {
        message.reply("No locks enabled :3").await?;
    }
    Ok(())
}

async fn handle_action(message: &Message, lockaction: ActionType) -> Result<()> {
    self_admin_or_die(message.get_chat_ref()).await?;

    if let Some(user) = message.get_from() {
        if user.is_admin(message.get_chat_ref()).await? {
            return Err(BotError::speak(
                "Premium user detected, but I can't ban an admin... cry.",
                message.get_chat().get_id(),
            ));
        }
        match lockaction {
            ActionType::Mute => {
                mute(message.get_chat_ref(), &user, None).await?;
                message.reply("Muted premium user").await?;
            }
            ActionType::Warn => {
                message
                    .reply("Warns not implemented, you lucky dawg")
                    .await?;
            }
            _ => (),
        };
    }
    TG.client()
        .build_delete_message(message.get_chat().get_id(), message.get_message_id())
        .build()
        .await?;
    Ok(())
}

async fn lock_action<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    let chat_id = message.get_chat().get_id();
    let lang = get_chat_lang(chat_id).await?;
    if let Some(arg) = args.args.first() {
        let action = ActionType::from_str_err(arg.get_text(), || {
            BotError::speak("Invalid action", chat_id)
        })?;
        set_default_action(message.get_chat_ref(), action).await?;
        message.reply(lang_fmt!(lang, "setdefaultaction")).await?;
    } else {
        message.reply(lang_fmt!(lang, "noactionarg")).await?;
    }
    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, _, args, message)) = ctx.cmd() {
        let command = ctx.command.as_ref();
        if is_premium(message) {
            if let Some(lock) = get_lock(message, LockType::Premium).await? {
                if let Some(action) = lock.lock_action {
                    handle_action(message, action).await?;
                } else {
                    let action = get_default_action(&message.get_chat()).await?;
                    handle_action(message, action).await?;
                }
            }
        }
        if let Some(user) = message.get_from() {
            match cmd {
                "lock" => handle_lock(message, &command, &user).await?,
                "unlock" => handle_unlock(message, &command, &user).await?,
                "locks" => handle_list(message, &user).await?,
                "lockaction" => lock_action(message, &args).await?,
                _ => (),
            };
        }
    }
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Context<'a>) -> Result<()> {
    handle_command(cmd).await
}
