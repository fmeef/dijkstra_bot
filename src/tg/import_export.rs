use std::collections::HashMap;

use botapi::gen_types::{
    EReplyMarkup, InlineKeyboardButtonBuilder, MaybeInaccessibleMessage, UpdateExt,
};
use chrono::Duration;
use futures::{future::BoxFuture, Future, FutureExt};
use macros::lang_fmt;
use redis::AsyncCommands;
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, TransactionTrait};
use sea_query::OnConflict;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    persist::{
        core::{
            media::{GetMediaId, MediaType},
            taint,
        },
        redis::{default_cache_query, ToRedisStr},
        redis::{CachedQueryTrait, RedisStr},
    },
    statics::{CONFIG, DB, ME, REDIS, TG},
    util::{
        error::{BotError, Result},
        string::Speak,
    },
};

use super::{
    admin_helpers::is_dm,
    button::{InlineKeyboardBuilder, OnPush},
    command::Context,
    markdown::EntityMessage,
};

#[derive(Serialize, Deserialize)]
pub struct RoseExport {
    pub bot_id: i64,
    pub data: HashMap<String, serde_json::Value>,
}

impl Default for RoseExport {
    fn default() -> Self {
        Self::new()
    }
}

impl RoseExport {
    pub fn new() -> Self {
        let bot_id = ME.get().unwrap().get_id();
        Self {
            bot_id,
            data: HashMap::new(),
        }
    }
}

#[inline(always)]
fn get_taint_key(media_id: &str) -> String {
    format!("tt:{}", media_id)
}

pub async fn is_tainted(media_id: &str, scope: &str, chat: i64) -> Result<bool> {
    let key = get_taint_key(media_id);
    let out = default_cache_query(
        |_, _| async move {
            Ok(taint::Entity::find()
                .filter(
                    taint::Column::MediaId.eq(media_id).and(
                        taint::Column::Scope
                            .eq(scope)
                            .and(taint::Column::Chat.eq(chat)),
                    ),
                )
                .one(*DB)
                .await?)
        },
        Duration::try_seconds(CONFIG.timing.cache_timeout).unwrap(),
    )
    .query(&key, &())
    .await?;

    Ok(out.is_some())
}

pub async fn set_taint(model: taint::Model) -> Result<()> {
    let key = get_taint_key(&model.media_id);
    let res = taint::Entity::insert(model.into_active_model())
        .on_conflict(
            OnConflict::columns([
                taint::Column::MediaId,
                taint::Column::Scope,
                taint::Column::Chat,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec_without_returning(*DB)
        .await?;
    if res > 0 {
        let _: () = REDIS.sq(|q| q.del(&key)).await?;
    }
    Ok(())
}

pub async fn set_taint_vec(media_id: Vec<taint::Model>) -> Result<()> {
    DB.transaction::<_, (), BotError>(|tx| {
        async move {
            // cry here, sea_orm doesn't support returning multiple rows via postgres
            // INSERT...RETURNING clause
            let existing = taint::Entity::find()
                .filter(
                    taint::Column::MediaId.is_not_in(media_id.iter().map(|v| v.media_id.as_str())),
                )
                .all(tx)
                .await?;
            let res = taint::Entity::insert_many(
                media_id.into_iter().map(|model| model.into_active_model()),
            )
            .on_conflict(
                OnConflict::columns([
                    taint::Column::MediaId,
                    taint::Column::Scope,
                    taint::Column::Chat,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(tx)
            .await?;
            if res > 0 {
                for key in existing {
                    let k = get_taint_key(&key.media_id);
                    let _: () = REDIS.sq(|q| q.del(&k)).await?;
                }
            }

            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn remove_taint(taint: &str) -> Result<()> {
    taint::Entity::delete_many()
        .filter(taint::Column::MediaId.eq(taint))
        .exec(*DB)
        .await?;

    let key = get_taint_key(taint);
    let _: () = REDIS.sq(|p| p.del(&key)).await?;

    Ok(())
}

pub async fn remove_taint_vec(taints: Vec<String>) -> Result<()> {
    taint::Entity::delete_many()
        .filter(taint::Column::MediaId.is_in(&taints))
        .exec(*DB)
        .await?;

    let _: () = REDIS
        .pipe(|p| {
            for taint in taints {
                let key = get_taint_key(&taint);
                p.del(&key);
            }
            p
        })
        .await?;
    Ok(())
}

#[inline(always)]
fn get_patch_taint_key(user: i64) -> String {
    format!("ptc:{}", user)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateTaint {
    pub media_id: String,
    pub media_type: MediaType,
    pub scope: String,
    pub chat: i64,
}

impl Context {
    pub async fn handle_taint<'a, F>(&'a self, scope: &str, cb: F) -> Result<()>
    where
        for<'b> F: FnOnce(&'b UpdateTaint, &'b str) -> BoxFuture<'b, Result<()>>,
    {
        if if let Some(chat) = self.chat() {
            is_dm(chat)
        } else {
            false
        } {
            if let UpdateExt::Message(ref message) = self.update() {
                if let Some(user) = message.get_from() {
                    let key = get_patch_taint_key(user.get_id());
                    let media_id: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
                    if let (Some(media_id), Some((new_media_id, new_media_type))) =
                        (media_id, message.get_media_id())
                    {
                        let taint: UpdateTaint = media_id.get()?;
                        if scope == taint.scope {
                            if taint.media_type != new_media_type {
                                self.reply(lang_fmt!(
                                    self,
                                    "wrongmediatype",
                                    new_media_type,
                                    taint.media_type
                                ))
                                .await?;
                                return Ok(());
                            }

                            log::info!("handle taint {} {}", taint.media_id, new_media_id);

                            cb(&taint, new_media_id).await?;
                            let _: () = REDIS.sq(|q| q.del(&key)).await?;
                            remove_taint(&taint.media_id).await?;
                            return Ok(());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn update_taint_id(&self, id: Uuid) -> Result<()> {
        let model = taint::Entity::find_by_id(id)
            .one(*DB)
            .await?
            .ok_or_else(|| {
                BotError::Generic("The missing media id specified does not exist".to_owned())
            })?;
        let message = self.message()?;
        if let Some(user) = message.get_from().map(|v| v.get_id()) {
            let ctx = UpdateTaint {
                media_id: model.media_id,
                media_type: model.media_type,
                scope: model.scope,
                chat: message.get_chat().get_id(),
            };

            log::info!("posting taint handler for {}", ctx.media_id);
            let key = get_patch_taint_key(user);
            let c = ctx.to_redis()?;
            let _: () = REDIS
                .pipe(|q| {
                    q.set(&key, c)
                        .expire(&key, Duration::try_minutes(45).unwrap().num_seconds())
                })
                .await?;
            self.reply(lang_fmt!(self, "taintforward", ctx.media_type))
                .await?;
        }

        Ok(())
    }

    /// Initiates a request to replace a "taintend" media id. Returns true if the user
    /// requested to delete the media, false if the user should forward the updated media
    /// to the bot's dm
    pub async fn update_taint<'a, F, Fut>(
        &self,
        scope: String,
        media_id: String,
        media_type: MediaType,
        cb: F,
    ) -> Result<()>
    where
        F: for<'b> FnOnce(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        if let Some(user) = self.message()?.get_from().map(|v| v.get_id()) {
            let mut buttons = InlineKeyboardBuilder::default();

            let delete = InlineKeyboardButtonBuilder::new(lang_fmt!(self, "taintdelete"))
                .set_callback_data(Uuid::new_v4().to_string())
                .build();

            let replace = InlineKeyboardButtonBuilder::new(lang_fmt!(self, "taintreplace"))
                .set_callback_data(Uuid::new_v4().to_string())
                .build();
            let id = media_id.clone();
            let taintmessage = lang_fmt!(self, "taintforward", media_type);
            replace.on_push(move |c| async move {
                if let Some(MaybeInaccessibleMessage::Message(message)) = c.get_message() {
                    TG.client
                        .build_edit_message_text(&taintmessage)
                        .message_id(message.get_message_id())
                        .chat_id(message.get_chat().get_id())
                        .build()
                        .await?;

                    let ctx = UpdateTaint {
                        media_id,
                        media_type,
                        scope: scope.to_owned(),
                        chat: message.get_chat().get_id(),
                    };

                    log::info!("posting taint handler for {}", ctx.media_id);
                    let key = get_patch_taint_key(user);
                    let ctx = ctx.to_redis()?;
                    let _: () = REDIS
                        .pipe(|q| {
                            q.set(&key, ctx)
                                .expire(&key, Duration::try_minutes(45).unwrap().num_seconds())
                        })
                        .await?;
                }

                Ok(())
            });

            delete.on_push(|c| async move {
                if let Some(MaybeInaccessibleMessage::Message(message)) = c.get_message() {
                    TG.client
                        .build_delete_message(message.get_chat().get_id(), message.get_message_id())
                        .build()
                        .await?;
                }
                cb(id).await?;
                Ok(())
            });

            buttons.button(delete);
            buttons.button(replace);

            self.reply_fmt(
                EntityMessage::from_text(
                    self.message()?.get_chat().get_id(),
                    lang_fmt!(self, "taintmessage"),
                )
                .reply_markup(EReplyMarkup::InlineKeyboardMarkup(buttons.build())),
            )
            .await?;
        }
        Ok(())
    }
}
