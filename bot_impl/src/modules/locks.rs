use self::entities::{default_locks, locks};
use crate::persist::admin::actions::ActionType;
use crate::persist::admin::approvals;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::{CONFIG, DB, REDIS};
use crate::tg::admin_helpers::ban_message;
use crate::tg::command::{Cmd, Context, TextArg, TextArgs};
use crate::tg::permissions::*;
use crate::tg::user::Username;
use crate::util::error::{BotError, Result};
use crate::util::string::{get_chat_lang, Lang};
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
    { command = "locks", help = "Get a list of active locks"},
    { command = "lockaction", help = "Set the action when a user sends a locked item"}
);

pub mod entities {
    use self::locks::LockAction;
    use super::Migration;
    use super::MigrationActionType;

    use crate::persist::admin::actions::ActionType;
    use crate::persist::migrate::ManagerHelper;
    use ::sea_orm_migration::prelude::*;
    use sea_orm::ActiveValue::{NotSet, Set};
    use sea_orm::ColumnTrait;
    use sea_orm::EntityTrait;
    use sea_orm::QueryFilter;

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
                        .modify_column(
                            ColumnDef::new(locks::Column::LockAction)
                                .default(None::<LockAction>)
                                .null()
                                .integer(),
                        )
                        .add_column(ColumnDef::new(locks::Column::Reason).text())
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(default_locks::Entity)
                        .col(
                            ColumnDef::new(default_locks::Column::Chat)
                                .big_integer()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(default_locks::Column::LockAction)
                                .integer()
                                .not_null()
                                .default(ActionType::Delete),
                        )
                        .col(ColumnDef::new(default_locks::Column::Duration).big_integer())
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            locks::Entity::update_many()
                .filter(locks::Column::LockAction.is_null())
                .set(locks::ActiveModel {
                    lock_type: NotSet,
                    lock_action: Set(Some(ActionType::Delete)),
                    chat: NotSet,
                    reason: NotSet,
                })
                .exec(manager.get_connection())
                .await?;
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
                        .drop_column(locks::Column::Reason)
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
        use sea_orm::ActiveValue::{NotSet, Set};

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
            pub duration: Option<i64>,
        }

        impl Model {
            pub fn default_from_chat(chat: i64) -> ActiveModel {
                ActiveModel {
                    chat: Set(chat),
                    lock_action: NotSet,
                    duration: NotSet,
                }
            }
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
            #[sea_orm(num_value = 2)]
            Link,
            #[sea_orm(num_value = 3)]
            Code,
            #[sea_orm(num_value = 4)]
            Photo,
            #[sea_orm(num_value = 5)]
            Video,
            #[sea_orm(num_value = 6)]
            AnonChannel,
            #[sea_orm(num_value = 7)]
            Command,
            #[sea_orm(num_value = 8)]
            Forward,
            #[sea_orm(num_value = 9)]
            Sticker,
        }

        impl LockType {
            pub fn get_name(&self) -> &str {
                match self {
                    Self::Premium => "Premium members",
                    Self::Link => "Web links",
                    Self::Code => "Monospace formatted pre code",
                    Self::Photo => "Photos",
                    Self::Video => "Videos",
                    Self::AnonChannel => "Users speaking through anonymous channels",
                    Self::Command => "Bot commands",
                    Self::Forward => "Forwarded messages",
                    Self::Sticker => "Stickers",
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
            pub reason: Option<String>,
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

macro_rules! locks {
    ( $( { $name:expr, $description:expr, $lock:expr, $predicate:expr } ),* ) => {

        static AVAILABLE_LOCKS: ::once_cell::sync::Lazy<::std::collections::HashMap<String, String>> =
                ::once_cell::sync::Lazy::new(|| {
           let mut map = ::std::collections::HashMap::new();
            $(
              map.insert($name.to_owned(), $description.to_owned());
            )+
            map
        });

        async fn action_from_update(
            update: &UpdateExt,
        ) -> Result<(Option<ActionType>, Vec<LockType>)> {
            let mut action: Option<ActionType> = None;
            let mut locks = Vec::<LockType>::new();
            match update {
                UpdateExt::Message(ref message) => {
                    $(
                    update_action(
                        message,
                        $lock,
                        &mut action,
                        &mut locks,
                        $predicate,
                    )
                    .await?;
                    )+
                }
                _ => (),
            }
            Ok((action, locks))
        }


        fn locktype_from_args<'a>(
            cmd: &Option<&'a Cmd<'a>>,
            chat: i64,
        ) -> (Option<LockType>, Option<ActionType>) {
            if let Some(&Cmd { ref args, .. }) = cmd {
                let action = args
                    .args
                    .get(1)
                    .map(|v| ActionType::from_str(v.get_text(), chat).ok())
                    .flatten();
                let arg = match args.args.first() {
                    $( Some(TextArg::Arg($name)) => Some($lock)),+,
                    _ => None,
                };

                (arg, action)
            } else {
                (None, None)
            }
        }
    };
}

locks! {
    {"code", "Pre formatted code", LockType::Code, |message| {
        if let Some(entities) = message.get_entities_ref() {
            for entity in entities {
                match entity.get_tg_type_ref() {
                    "pre" => return true,
                    "code" => return true,
                    _ => (),
                }
            }
        }
        false

    }},
    {"premium", "Messages from premium users", LockType::Premium, |message| {
       if let Some(user) = message.get_from() {
            user.get_is_premium().unwrap_or(false)
        } else {
            false
        }
    }},
    {"url", "http/https urls, as defined by telegram", LockType::Link, |message| {
        if let Some(entities) = message.get_entities_ref() {
            for entity in entities {
                match entity.get_tg_type_ref() {
                    "url" => return true,
                    "text_link" => return true,
                    _ => (),
                }
            }
        }
        false
    }},
    {"photo", "Photo messages", LockType::Photo, |message| {
        message.get_photo().is_some()
    }},
    {"video", "Video messages", LockType::Video, |message| {
        message.get_video().is_some()
    }},
    {"anonchannel", "Users speaking through anonymous channels", LockType::AnonChannel, |message| {
        message.get_sender_chat().is_some()
    }},
    {"command", "Bot commands", LockType::Command, |message| {
        if let Some(entities) = message.get_entities_ref() {
            for entity in entities {
                match entity.get_tg_type_ref() {
                    "bot_command" => return true,
                    _ => (),
                }
            }
        }
        false
    }},
    {"forward", "Forwarded messages", LockType::Forward, |message| {
        message.get_forward_from().is_some()
    }},
    {"sticker", "Stickers", LockType::Sticker, |message| message.get_sticker().is_some() }
}

pub fn get_lock_key(chat: i64, locktype: &LockType) -> String {
    format!("lock:{}:{}", chat, locktype.get_name())
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
                .one(DB.deref())
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
        .exec(DB.deref())
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
        reason: NotSet,
    };
    let res = locks::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([locks::Column::Chat, locks::Column::LockType])
                .update_column(locks::Column::LockAction)
                .to_owned(),
        )
        .exec_with_returning(DB.deref())
        .await?;
    res.cache(key).await?;
    Ok(())
}

#[inline(always)]
fn get_default_key(chat: &Chat) -> String {
    format!("daction:{}", chat.get_id())
}

async fn get_default_settings(chat: &Chat) -> Result<default_locks::Model> {
    let chat_id = chat.get_id();
    let key = get_default_key(chat);
    default_cache_query(
        |_, _| async move {
            let model =
                default_locks::Entity::insert(default_locks::Model::default_from_chat(chat_id))
                    .on_conflict(
                        OnConflict::column(default_locks::Column::Chat)
                            .update_column(default_locks::Column::Chat)
                            .to_owned(),
                    )
                    .exec_with_returning(DB.deref())
                    .await?;
            Ok(Some(model))
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await
    .map(|v| v.expect("this should't happen"))
}

async fn set_default_action(chat: &Chat, lock_action: ActionType) -> Result<()> {
    let model = default_locks::Model {
        chat: chat.get_id(),
        lock_action,
        duration: None,
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
) -> Result<()> {
    let key = get_lock_key(message.get_chat().get_id(), &locktype);
    let model = locks::Model {
        chat: message.get_chat().get_id(),
        lock_type: locktype,
        lock_action: Some(lockaction),
        reason: None,
    };
    locks::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::columns([locks::Column::Chat, locks::Column::LockType])
                .update_column(locks::Column::LockAction)
                .to_owned(),
        )
        .exec(DB.deref())
        .await?;
    Ok(())
}

async fn handle_lock<'a>(
    message: &Message,
    cmd: &Option<&Cmd<'a>>,
    user: &User,
    lang: &Lang,
) -> Result<()> {
    message
        .check_permissions(|p| p.can_delete_messages.and(p.can_change_info))
        .await?;
    match locktype_from_args(cmd, message.get_chat().get_id()) {
        (Some(lock), None) => {
            let t = lock.get_name().to_owned();

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
            set_lock_action(message, lock, action).await?;
            message.reply(reply).await?;
        }
        _ => {
            message.reply(lang_fmt!(lang, "locknotspec")).await?;
        }
    };
    Ok(())
}

async fn handle_unlock<'a>(ctx: &Context, cmd: &Option<&Cmd<'a>>) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let message = ctx.message()?;
    let lang = ctx.lang();
    if let (Some(lock), _) = locktype_from_args(cmd, message.get_chat().get_id()) {
        let name = lock.get_name().to_owned();
        clear_lock(message, lock).await?;
        message.reply(lang_fmt!(lang, "clearedlock", name)).await?;
    } else {
        message.reply(lang_fmt!(lang, "locknotspec")).await?;
    }
    Ok(())
}

async fn handle_list(message: &Message) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    let chat = message.get_chat().get_id();
    let locks = locks::Entity::find()
        .filter(locks::Column::Chat.eq(chat))
        .all(DB.deref())
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

async fn lock_action<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
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

#[inline(always)]
fn get_approval_key(chat: &Chat, user: &User) -> String {
    format!("ap:{}:{}", chat.get_id(), user.get_id())
}

async fn is_approved(chat: &Chat, user: &User) -> Result<bool> {
    let chat_id = chat.get_id();
    let user_id = user.get_id();
    let key = get_approval_key(chat, user);
    let res = default_cache_query(
        |_, _| async move {
            let res = approvals::Entity::find_by_id((chat_id, user_id))
                .one(DB.deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?
    .is_some();

    Ok(res)
}

async fn cmd_available(ctx: &Context) -> Result<()> {
    let available = ["[*Available locks]:".to_owned()]
        .into_iter()
        .chain(
            AVAILABLE_LOCKS
                .iter()
                .map(|(name, desc)| format!("[`{}:] {}", name, desc)),
        )
        .collect::<Vec<String>>()
        .join("\n");
    ctx.message()?.speak(available).await?;
    Ok(())
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        lang,
        ..
    }) = ctx.cmd()
    {
        let command = ctx.try_get()?.command.as_ref();
        if let Some(user) = message.get_from() {
            match cmd {
                "lock" => handle_lock(message, &command, &user, lang).await?,
                "unlock" => handle_unlock(ctx, &command).await?,
                "locks" => handle_list(message).await?,
                "lockaction" => lock_action(message, &args).await?,
                "available" => cmd_available(ctx).await?,
                _ => (),
            };
        }
    }
    Ok(())
}

#[inline(always)]
async fn update_action<F>(
    message: &Message,
    locktype: LockType,
    action: &mut Option<ActionType>,
    locks: &mut Vec<LockType>,
    p: F,
) -> Result<()>
where
    F: for<'b> FnOnce(&'b Message) -> bool,
{
    if p(message) {
        if let Some(newaction) = get_lock(message, locktype.clone()).await? {
            let newaction = if let Some(action) = newaction.lock_action {
                Some(action)
            } else {
                Some(
                    get_default_settings(message.get_chat_ref())
                        .await?
                        .lock_action,
                )
            };

            if newaction > *action {
                *action = newaction;
            }
            log::info!("encountered locked media! {}", locktype.get_name());
            locks.push(locktype);
        }
    }

    Ok(())
}

async fn handle_user_event(update: &UpdateExt, ctx: &Context) -> Result<()> {
    if let (Some(action), locks) = action_from_update(update).await? {
        match update {
            UpdateExt::Message(ref message) => {
                if let Some(user) = message.get_from_ref() {
                    if is_approved(message.get_chat_ref(), user).await? {
                        return Ok(());
                    }
                }
                if message.get_from().is_admin(message.get_chat_ref()).await? {
                    return Ok(());
                }
                let default = get_default_settings(message.get_chat_ref()).await?;
                let lang = ctx.try_get()?.lang;
                let reasons = locks
                    .into_iter()
                    .map(|v| lang_fmt!(lang, "lockedinchat", v.get_name()))
                    .collect::<Vec<String>>()
                    .join("\n");

                match action {
                    ActionType::Delete => {}
                    ActionType::Ban => {
                        ban_message(&message, default.duration.map(|v| Duration::seconds(v)))
                            .await?;
                        message.speak(lang_fmt!(lang, "lockban", reasons)).await?;
                    }
                    ActionType::Warn => {
                        if let Some(chat) = message.get_sender_chat() {
                            TG.client
                                .build_ban_chat_sender_chat(
                                    message.get_chat().get_id(),
                                    chat.get_id(),
                                )
                                .build()
                                .await?;
                        } else if let Some(user) = message.get_from() {
                            ctx.warn_with_action(
                                user.get_id(),
                                Some(&reasons),
                                default.duration.map(|v| Duration::seconds(v)),
                            )
                            .await?;
                        }
                    }
                    _ => (),
                }

                TG.client
                    .build_delete_message(message.get_chat().get_id(), message.get_message_id())
                    .build()
                    .await?;
            }
            _ => (),
        }
    }
    Ok(())
}

pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_user_event(cmd.update(), cmd).await?;
    handle_command(cmd).await?;

    Ok(())
}
