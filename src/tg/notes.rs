//! Because notes (key/value text or media fetchable via /get notename or #notename) are referenced outside
//! of the notes module itself (ie. via button menus), notes are a core feature of the bot framework.
//!
//! this module has helper functions for storing, retrieving, and printing notes

use std::collections::BTreeMap;

use botapi::gen_types::{CallbackQuery, MaybeInaccessibleMessage, MessageEntity};
use futures::{future::BoxFuture, FutureExt};
use itertools::Itertools;
use redis::AsyncCommands;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, TransactionTrait};

use crate::{
    persist::{
        core::{entity, media::SendMediaReply, notes},
        redis::{CachedQuery, CachedQueryTrait, RedisStr},
    },
    statics::{CONFIG, DB, REDIS, TG},
    tg::button::OnPush,
    util::error::{BotError, Result},
};

use super::{button::InlineKeyboardBuilder, command::Context, markdown::get_markup_for_buttons};

pub const MODULE_NAME: &str = "notes";

#[inline(always)]
pub(crate) fn get_hash_key(chat: i64) -> String {
    format!("ncch:{}", chat)
}

pub async fn refresh_notes(
    chat: i64,
) -> Result<
    BTreeMap<
        String,
        (
            notes::Model,
            Vec<MessageEntity>,
            Option<InlineKeyboardBuilder>,
        ),
    >,
> {
    let hash_key = get_hash_key(chat);
    let (exists, notes): (bool, BTreeMap<String, RedisStr>) = REDIS
        .pipe(|q| q.exists(&hash_key).hgetall(&hash_key))
        .await?;

    if !exists {
        let notes = notes::get_filters_join(notes::Column::Chat.eq(chat))
            .await?
            .into_iter()
            .map(|(note, (entity, button))| {
                (
                    note,
                    entity
                        .into_iter()
                        .map(|e| e.get())
                        .map(|(e, u)| e.to_entity(u))
                        .collect(),
                    get_markup_for_buttons(button.into_iter().collect()),
                )
            })
            .collect_vec();
        let st = notes
            .iter()
            .filter_map(|v| RedisStr::new(&v).ok().map(|s| (v.0.name.clone(), s)))
            .collect_vec();
        let _: () = REDIS
            .pipe(|q| {
                if !st.is_empty() {
                    q.hset_multiple(&hash_key, st.as_slice());
                }
                q.expire(&hash_key, CONFIG.timing.cache_timeout)
            })
            .await?;

        Ok(notes.into_iter().map(|v| (v.0.name.clone(), v)).collect())
    } else {
        Ok(notes
            .into_iter()
            .filter_map(|(n, v)| v.get().ok().map(|v| (n, v)))
            .collect())
    }
}

pub async fn clear_notes(chat: i64) -> Result<()> {
    let key = get_hash_key(chat);
    DB.transaction::<_, (), BotError>(|tx| {
        async move {
            let ids: Vec<Option<i64>> = notes::Entity::find()
                .select_only()
                .filter(notes::Column::Chat.eq(chat))
                .columns([notes::Column::EntityId])
                .into_tuple()
                .all(tx)
                .await?;
            notes::Entity::delete_many()
                .filter(notes::Column::Chat.eq(chat))
                .exec(tx)
                .await?;

            entity::Entity::delete_many()
                .filter(entity::Column::Id.is_in(ids))
                .exec(tx)
                .await?;
            let _: () = REDIS.sq(|q| q.del(key)).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn get_note_by_name(
    name: String,
    chat: i64,
) -> Result<
    Option<(
        notes::Model,
        Vec<MessageEntity>,
        Option<InlineKeyboardBuilder>,
    )>,
> {
    let hash_key = get_hash_key(chat);
    let n = name.clone();
    let note = CachedQuery::new(
        |_, _| async move {
            let res = notes::get_filters_join(
                notes::Column::Name.eq(n).and(notes::Column::Chat.eq(chat)),
            )
            .await?;

            Ok(res
                .into_iter()
                .map(|(note, (entity, button))| {
                    log::info!("note from database {:?}", button);
                    (
                        note,
                        entity
                            .into_iter()
                            .map(|e| e.get())
                            .map(|(e, u)| e.to_entity(u))
                            .collect(),
                        get_markup_for_buttons(button.into_iter().collect()),
                    )
                })
                .next())
        },
        |key, _| async move {
            let (exists, key, _): (bool, Option<RedisStr>, ()) = REDIS
                .pipe(|q| {
                    q.exists(&hash_key)
                        .hget(&hash_key, key)
                        .expire(&hash_key, CONFIG.timing.cache_timeout)
                })
                .await?;

            let res = if let Some(key) = key {
                Some(key.get()?)
            } else {
                None
            };

            Ok((exists, res))
        },
        |_, value| async move {
            refresh_notes(chat).await?;
            Ok(value)
        },
    )
    .query(&name, &())
    .await?;
    Ok(note)
}

/// Handles a note button transition
pub fn handle_transition(
    ctx: &Context,
    chat: i64,
    note: String,
    callback: CallbackQuery,
) -> BoxFuture<'_, Result<()>> {
    async move {
        log::info!("current note: {}", note);
        if let Some((note, extra_entities, extra_buttons)) = get_note_by_name(note, chat).await? {
            let c = ctx.clone();
            if let MaybeInaccessibleMessage::Message(message) = callback
                .get_message()
                .ok_or_else(|| BotError::Generic("message missing".to_owned()))?
            {
                SendMediaReply::new(ctx, note.media_type)
                    .button_callback(move |note, button| {
                        let c = c.clone();
                        async move {
                            log::info!("next notes: {}", note);
                            button.on_push(move |b| async move {
                                TG.client
                                    .build_answer_callback_query(b.get_id())
                                    .build()
                                    .await?;

                                handle_transition(&c, chat, note, b).await?;
                                Ok(())
                            });

                            Ok(())
                        }
                        .boxed()
                    })
                    .text(note.text)
                    .media_id(note.media_id)
                    .extra_entities(extra_entities)
                    .buttons(extra_buttons)
                    .edit_media_reply_chatuser(message)
                    .await?;
            }
        }

        Ok(())
    }
    .boxed()
}
