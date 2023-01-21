use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisStr};
use crate::statics::{DB, REDIS, TG};
use anyhow::Result;

use async_trait::async_trait;
use botapi::gen_types::{Chat, Message};
use chrono::Duration;
use lazy_static::__Deref;
use macros::get_langs;

get_langs!();

pub use langs::*;

use redis::AsyncCommands;
use sea_orm::sea_query::OnConflict;
use sea_orm::{prelude::ChronoDateTimeWithTimeZone, EntityTrait, IntoActiveModel};

use crate::persist::core::dialogs;

#[allow(dead_code)]
fn get_query<'r>() -> impl CachedQueryTrait<'r, Lang, i64> {
    default_cache_query(
        |_, chat| async move {
            let chat: &i64 = chat;
            Ok(dialogs::Entity::find_by_id(*chat)
                .one(DB.deref())
                .await?
                .map(|v| v.language)
                .unwrap_or_else(|| Lang::En))
        },
        Duration::hours(12),
    )
}

pub async fn should_ignore_chat(chat: i64) -> Result<bool> {
    let key = format!("ign:{}", chat);
    let ignore = REDIS.sq(|q| q.exists(&key)).await?;
    Ok(ignore)
}

pub async fn ignore_chat(chat: i64, time: &Duration) -> Result<()> {
    let key = format!("ign:{}", chat);
    REDIS
        .pipe(|q| q.set(&key, true).expire(&key, time.num_seconds() as usize))
        .await?;
    Ok(())
}

#[async_trait]
pub trait Speak {
    async fn speak<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync;
    async fn reply<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync;
}

#[async_trait]
impl Speak for Message {
    async fn speak<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            TG.client()
                .build_send_message(self.get_chat().get_id(), message.as_ref())
                .build()
                .await?;
        }
        Ok(())
    }
    async fn reply<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            TG.client()
                .build_send_message(self.get_chat().get_id(), message.as_ref())
                .reply_to_message_id(self.get_message_id())
                .build()
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Speak for Chat {
    async fn speak<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_id()).await? {
            TG.client()
                .build_send_message(self.get_id(), message.as_ref())
                .build()
                .await?;
        }
        Ok(())
    }
    async fn reply<T>(&self, message: T) -> Result<()>
    where
        T: AsRef<str> + Send + Sync,
    {
        self.speak(message).await
    }
}

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
