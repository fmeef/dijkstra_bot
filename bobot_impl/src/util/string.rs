use crate::persist::redis::{default_cache_query, static_query, RedisStr};
use crate::statics::{DB, REDIS};
use anyhow::Result;

use chrono::Duration;
use lazy_static::__Deref;
use macros::get_langs;

get_langs!();

pub use langs::*;

use sea_orm::sea_query::OnConflict;
use sea_orm::{prelude::ChronoDateTimeWithTimeZone, EntityTrait, IntoActiveModel};

use crate::persist::core::dialogs;

static_query! {
    static ref QUERY_NEW: ( i64 => Lang ) =
default_cache_query(
        |_, chat| async move {
            Ok(dialogs::Entity::find_by_id(chat)
                .one(DB.deref())
                .await?
                .map(|v| v.language)
                .unwrap_or_else(|| Lang::En))
        },
        Duration::hours(12),
    )
}

fn get_lang_key(chat: i64) -> String {
    format!("lang:{}", chat)
}

pub async fn get_chat_lang(chat: i64) -> Result<Lang> {
    let key = get_lang_key(chat);
    let res = QUERY_NEW.query(key, chat).await?;
    Ok(res)
}

pub async fn set_chat_lang(chat: i64, lang: Lang) -> Result<()> {
    let r = RedisStr::new(&lang)?;
    let c = dialogs::Model {
        chat_id: chat,
        last_activity: ChronoDateTimeWithTimeZone::default(),
        language: lang,
    };
    let key = get_lang_key(chat);
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
