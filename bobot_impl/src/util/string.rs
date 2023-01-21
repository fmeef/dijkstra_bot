use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisStr};
use crate::statics::{DB, REDIS};
use anyhow::Result;

use botapi::gen_types::Chat;
use chrono::Duration;
use lazy_static::__Deref;
use macros::get_langs;
use sea_orm::DatabaseConnection;
use serde::{de::DeserializeOwned, Serialize};
get_langs!();

pub use langs::*;

use sea_orm::sea_query::OnConflict;
use sea_orm::{prelude::ChronoDateTimeWithTimeZone, EntityTrait, IntoActiveModel};

use crate::persist::core::dialogs;
/*
fn get_query<'r, T, P>() -> Box<dyn CachedQueryTrait<'r, T, P>>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'r,
    P: Send + Sync + 'r,
{
    let res = default_cache_query(
        |_, chat| async move {
            let chat: &i64 = chat;
            Ok(dialogs::Entity::find_by_id(*chat)
                .one(DB.deref())
                .await?
                .map(|v| v.language)
                .unwrap_or_else(|| Lang::En))
        },
        Duration::hours(12),
    );
    Box::new(res)
}
*/

fn get_lang_key(chat: i64) -> String {
    format!("lang:{}", chat)
}

pub async fn get_chat_lang(chat: i64) -> Result<Lang> {
    let key = get_lang_key(chat);
    let res = default_cache_query(
        |_, _| async move {
            Ok(dialogs::Entity::find_by_id(chat)
                .one(DB.deref())
                .await?
                .map(|v| v.language)
                .unwrap_or_else(|| Lang::En))
        },
        Duration::hours(12),
    )
    .query(&key, &())
    .await?;
    Ok(res)
}

pub async fn set_chat_lang(chat: &Chat, lang: Lang) -> Result<()> {
    let r = RedisStr::new(&lang)?;
    let c = dialogs::Model {
        chat_id: chat.get_id(),
        last_activity: ChronoDateTimeWithTimeZone::default(),
        language: lang,
        chat_type: chat.get_tg_type().to_owned(),
        warn_limit: None,
    };
    let key = get_lang_key(chat.get_id());
    REDIS
        .pipe(|p| {
            p.set(&key, r)
                .expire(&key, Duration::hours(12).num_seconds() as usize)
        })
        .await?;
    dialogs::Entity::insert(c.into_active_model())
        .on_conflict(
            OnConflict::column(dialogs::Column::Language)
                .update_column(dialogs::Column::Language)
                .to_owned(),
        )
        .exec(DB.deref().deref())
        .await?;

    Ok(())
}
