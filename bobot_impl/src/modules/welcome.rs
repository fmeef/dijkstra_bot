use crate::persist::core::media::{get_media_type, send_media_reply, MediaType};
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::{CONFIG, DB, REDIS};
use crate::tg::admin_helpers::IsGroupAdmin;
use crate::tg::command::{Context, TextArgs};
use crate::util::error::{BotError, Result};

use crate::util::string::get_chat_lang;
use crate::{metadata::metadata, util::string::Speak};
use botapi::gen_types::{Message, UpdateExt, User};
use chrono::Duration;
use lazy_static::__Deref;

use macros::lang_fmt;
use redis::AsyncCommands;
use sea_orm::entity::ActiveValue::{NotSet, Set};
use sea_orm::EntityTrait;
use sea_orm_migration::{MigrationName, MigrationTrait};
use sea_query::OnConflict;

metadata!("Welcome",
    r#"
    Welcomes users with custom message
    "#,
    { command = "welcome", help = "Usage: welcome <on/off>. Enables or disables welcome" },
    { command = "setwelcome", help = "Sets the welcome text. Reply to a message or media to set"}
);

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230312_000001_create_welcomes"
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
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
                        .table(welcomes::Entity)
                        .col(
                            ColumnDef::new(welcomes::Column::Chat)
                                .big_integer()
                                .primary_key(),
                        )
                        .col(ColumnDef::new(welcomes::Column::Text).text())
                        .col(ColumnDef::new(welcomes::Column::MediaId).text())
                        .col(ColumnDef::new(welcomes::Column::MediaType).integer())
                        .col(ColumnDef::new(welcomes::Column::GoodbyeText).text())
                        .col(ColumnDef::new(welcomes::Column::GoodbyeMediaId).text())
                        .col(ColumnDef::new(welcomes::Column::GoodbyeMediaType).integer())
                        .col(
                            ColumnDef::new(welcomes::Column::Enabled)
                                .boolean()
                                .not_null()
                                .default(false),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
            manager.drop_table_auto(welcomes::Entity).await?;

            Ok(())
        }
    }
    pub mod welcomes {
        use crate::persist::core::media::*;
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};
        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "welcome")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub chat: i64,
            #[sea_orm(column_type = "Text")]
            pub text: Option<String>,
            pub media_id: Option<String>,
            pub media_type: Option<MediaType>,
            #[sea_orm(column_type = "Text")]
            pub goodbye_text: Option<String>,
            pub goodbye_media_id: Option<String>,
            pub goodbye_media_type: Option<MediaType>,
            #[sea_orm(default = false)]
            pub enabled: bool,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}
        impl Related<super::welcomes::Entity> for Entity {
            fn to() -> RelationDef {
                panic!("no relations")
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

fn get_model<'a>(
    message: &'a Message,
    args: &'a TextArgs<'a>,
    goodbye: bool,
) -> Result<entities::welcomes::ActiveModel> {
    let (message, text) = if let Some(message) = message.get_reply_to_message_ref() {
        (message, message.get_text_ref())
    } else {
        (message, Some(args.text))
    };
    let (media_id, media_type) = get_media_type(message)?;
    let res = if goodbye {
        entities::welcomes::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            text: NotSet,
            media_id: NotSet,
            media_type: NotSet,
            goodbye_text: Set(text.map(|t| t.to_owned())),
            goodbye_media_id: Set(media_id),
            goodbye_media_type: Set(Some(media_type)),
            enabled: NotSet,
        }
    } else {
        entities::welcomes::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            text: Set(text.map(|t| t.to_owned())),
            media_id: Set(media_id),
            media_type: Set(Some(media_type)),
            goodbye_text: NotSet,
            goodbye_media_id: NotSet,
            goodbye_media_type: NotSet,
            enabled: NotSet,
        }
    };

    Ok(res)
}

async fn enable_welcome<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    let enabled = match args.args.first().map(|v| v.get_text()) {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some("yes") => Ok(true),
        Some("no") => Ok(false),
        _ => Err(BotError::speak(
            lang_fmt!(lang, "welcomeinvalid"),
            message.get_chat().get_id(),
        )),
    }?;
    let model = entities::welcomes::ActiveModel {
        chat: Set(message.get_chat().get_id()),
        text: NotSet,
        media_id: NotSet,
        media_type: NotSet,
        goodbye_text: NotSet,
        goodbye_media_id: NotSet,
        goodbye_media_type: NotSet,
        enabled: Set(enabled),
    };

    let model = entities::welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::column(entities::welcomes::Column::Chat)
                .update_column(entities::welcomes::Column::Enabled)
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;
    model.cache(key).await?;
    message.reply("Enabled welcome").await?;
    Ok(())
}

async fn should_welcome(message: &Message) -> Result<Option<entities::welcomes::Model>> {
    let key = format!("welcome:{}", message.get_chat().get_id());
    let chat_id = message.get_chat().get_id();
    let res = default_cache_query(
        |_, _| async move {
            let res = entities::welcomes::Entity::find_by_id(chat_id)
                .one(DB.deref())
                .await?;
            Ok(res)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

async fn set_goodbye<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message.group_admin_or_die().await?;
    let model = get_model(message, args, true)?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    log::info!("save goodbye: {}", key);
    let model = entities::welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([entities::welcomes::Column::Chat])
                .update_columns([
                    entities::welcomes::Column::GoodbyeText,
                    entities::welcomes::Column::GoodbyeMediaId,
                    entities::welcomes::Column::GoodbyeMediaType,
                ])
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;
    let text = if let Some(text) = model.text.as_ref() {
        lang_fmt!(lang, "setgoodbye", text)
    } else {
        lang_fmt!(lang, "setgoodbye", "*media*")
    };
    model.cache(key).await?;

    message.speak(text).await?;
    Ok(())
}

async fn set_welcome<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message.group_admin_or_die().await?;

    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let model = get_model(message, args, false)?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    log::info!("save welcome: {}", key);
    let model = entities::welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([entities::welcomes::Column::Chat])
                .update_columns([
                    entities::welcomes::Column::Text,
                    entities::welcomes::Column::MediaId,
                    entities::welcomes::Column::MediaType,
                ])
                .to_owned(),
        )
        .exec_with_returning(DB.deref().deref())
        .await?;

    let text = if let Some(text) = model.text.as_ref() {
        lang_fmt!(lang, "setwelcome", text)
    } else {
        lang_fmt!(lang, "setwelcome", "*media*")
    };
    model.cache(key).await?;
    message.speak(text).await?;
    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, _, args, message)) = ctx.cmd() {
        match cmd {
            "setwelcome" => set_welcome(message, args).await?,
            "setgoodbye" => set_goodbye(message, args).await?,
            "welcome" => enable_welcome(message, args).await?,
            "resetwelcome" => reset_welcome(message).await?,
            _ => (),
        };
    }
    Ok(())
}

async fn reset_welcome(message: &Message) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let chat = message.get_chat().get_id();
    let key = format!("welcome:{}", chat);

    entities::welcomes::Entity::delete_by_id(chat)
        .exec(DB.deref())
        .await?;
    REDIS.sq(|q| q.del(&key)).await?;
    message.speak(lang_fmt!(lang, "resetwelcome")).await?;
    Ok(())
}

async fn welcome_mambers(
    message: Message,
    mambers: Vec<User>,
    model: entities::welcomes::Model,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let text = if let Some(text) = model.text {
        text
    } else {
        lang_fmt!(lang, "defaultwelcome")
    };
    for _ in mambers.iter() {
        send_media_reply(
            &message,
            model.media_type.clone().unwrap_or(MediaType::Text),
            Some(text.clone()),
            model.media_id.clone(),
        )
        .await?;
        if mambers.len() > 1 {
            tokio::time::sleep(Duration::seconds(2).to_std()?).await;
        }
    }
    Ok(())
}

async fn goodbye_mambers(
    message: Message,
    _mambers: User,
    model: entities::welcomes::Model,
) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let text = if let Some(text) = model.goodbye_text {
        text
    } else {
        lang_fmt!(lang, "defaultgoodbye")
    };
    send_media_reply(
        &message,
        model.goodbye_media_type.unwrap_or(MediaType::Text),
        Some(text),
        model.goodbye_media_id,
    )
    .await?;
    Ok(())
}

pub async fn handle_update<'a>(update: &UpdateExt, cmd: &Context<'a>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => {
            if let Some(model) = should_welcome(message).await? {
                if model.enabled {
                    if let Some(mambers) = message.get_left_chat_member() {
                        let mamber = mambers.into_owned();
                        let message = message.to_owned();
                        let model = model.clone();
                        tokio::spawn(async move {
                            if let Err(err) = goodbye_mambers(message, mamber, model).await {
                                log::error!("goodbye mambers error: {}", err);
                                err.record_stats();
                            }
                        });
                    }
                    if let Some(mambers) = message.get_new_chat_members() {
                        let mambers = mambers.into_owned();
                        let message = message.to_owned();
                        tokio::spawn(async move {
                            if let Err(err) = welcome_mambers(message, mambers, model).await {
                                log::error!("welcome mambers error: {}", err);
                                err.record_stats();
                            }
                        });
                    }
                }
            }
        }
        _ => (),
    };
    handle_command(cmd).await
}
