use self::entities::locks;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::{CONFIG, DB, REDIS};
use crate::tg::admin_helpers::{change_permissions, self_admin_or_die, IsAdmin};
use crate::tg::command::{Command, TextArg};
use crate::util::error::{BotError, Result};
use crate::{metadata::metadata, statics::TG, util::string::Speak};
use botapi::gen_types::{ChatPermissionsBuilder, Message, UpdateExt, User};
use chrono::Duration;
use entities::locks::{LockAction, LockType};
use lazy_static::__Deref;

use redis::AsyncCommands;
use sea_orm::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::EntityTrait;
use sea_orm_migration::{MigrationName, MigrationTrait};

metadata!("Locks",
    { command = "lock", help = "Engage a lock" },
    { command = "unlock", help = "Disable a lock"},
    { command = "locks", help = "Get a list of active locks"}
);

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;

    use self::locks::LockAction;

    use super::Migration;

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

    pub mod locks {
        use sea_orm::{entity::prelude::*, TryFromU64};
        use serde::{Deserialize, Serialize};

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

        impl TryFromU64 for LockType {
            fn try_from_u64(n: u64) -> Result<Self, DbErr> {
                match n {
                    1 => Ok(Self::Premium),
                    _ => Err(DbErr::ConvertFromU64("cry")),
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "locks")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(primary_key)]
            pub lock_type: LockType,
            #[sea_orm(default = LockAction::Silent)]
            pub lock_action: LockAction,
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

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230129_000001_create_locks"
    }
}

pub fn get_lock_key(chat: i64, locktype: &LockType) -> String {
    format!("lock:{}:{}", chat, locktype.to_string())
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
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
        chat: sea_orm::ActiveValue::Set(message.get_chat().get_id()),
        lock_type: sea_orm::ActiveValue::Set(locktype),
        lock_action: sea_orm::ActiveValue::NotSet,
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

#[allow(dead_code)]
async fn set_lock_action(
    message: &Message,
    locktype: LockType,
    lockaction: LockAction,
    user: &User,
) -> Result<()> {
    user.admin_or_die(message.get_chat_ref()).await?;
    let key = get_lock_key(message.get_chat().get_id(), &locktype);
    let model = locks::Model {
        chat: message.get_chat().get_id(),
        lock_type: locktype,
        lock_action: lockaction,
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

fn locktype_from_args<'a>(cmd: &Option<&'a Command<'a>>) -> Option<LockType> {
    if let Some(&Command { ref args, .. }) = cmd {
        match args.args.first() {
            Some(TextArg::Arg("premium")) => Some(LockType::Premium),
            _ => None,
        }
    } else {
        None
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
    user.admin_or_die(message.get_chat_ref()).await?;
    if let Some(lock) = locktype_from_args(cmd) {
        set_lock(message, lock, user).await?;
        message.reply("Set lock").await?;
    } else {
        message.reply("Specify a lock").await?;
    }
    Ok(())
}

async fn handle_unlock<'a>(
    message: &Message,
    cmd: &Option<&Command<'a>>,
    user: &User,
) -> Result<()> {
    user.admin_or_die(message.get_chat_ref()).await?;
    if let Some(lock) = locktype_from_args(cmd) {
        clear_lock(message, lock).await?;
        message.reply("Cleared lock").await?;
    } else {
        message.reply("Specify a lock").await?;
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

async fn handle_action(message: &Message, lockaction: LockAction) -> Result<()> {
    self_admin_or_die(message.get_chat_ref()).await?;

    if let Some(user) = message.get_from() {
        if user.is_admin(message.get_chat_ref()).await? {
            return Err(BotError::speak(
                "Premium user detected, but I can't ban an admin... cry.",
                message.get_chat().get_id(),
            ));
        }
        match lockaction {
            LockAction::Mute => {
                let permissions = ChatPermissionsBuilder::new()
                    .set_can_send_messages(false)
                    .set_can_send_audios(false)
                    .set_can_send_documents(false)
                    .set_can_send_photos(false)
                    .set_can_send_videos(false)
                    .set_can_send_video_notes(false)
                    .set_can_send_polls(false)
                    .set_can_send_voice_notes(false)
                    .set_can_send_other_messages(false)
                    .build();
                change_permissions(message, &user, &permissions, None).await?;
                message.reply("Muted premium user").await?;
            }
            LockAction::Warn => {
                message
                    .reply("Warns not implemented, you lucky dawg")
                    .await?;
            }
            LockAction::Silent => (),
        };
    }
    TG.client()
        .build_delete_message(message.get_chat().get_id(), message.get_message_id())
        .build()
        .await?;
    Ok(())
}

async fn handle_command<'a>(message: &Message, command: Option<&'a Command<'a>>) -> Result<()> {
    if is_premium(message) {
        if let Some(lock) = get_lock(message, LockType::Premium).await? {
            handle_action(message, lock.lock_action).await?;
        }
    }
    if let Some(&Command { cmd, .. }) = command {
        if let Some(user) = message.get_from() {
            match cmd {
                "lock" => handle_lock(message, &command, &user).await?,
                "unlock" => handle_unlock(message, &command, &user).await?,
                "locks" => handle_list(message, &user).await?,
                _ => (),
            };
        }
    }
    Ok(())
}

pub async fn handle_update<'a>(update: &UpdateExt, cmd: Option<&'a Command<'a>>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message, cmd).await?,
        _ => (),
    };
    Ok(())
}
