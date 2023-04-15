use std::ops::DerefMut;

use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisStr};
use crate::statics::{CONFIG, DB, REDIS, TG};
use crate::tg::admin_helpers::IntoChatUser;
use crate::tg::markdown::MarkupBuilder;
use crate::util::error::Result;

use async_trait::async_trait;
use botapi::gen_methods::CallSendMessage;
use botapi::gen_types::{Chat, Message};
use chrono::Duration;
use lazy_static::__Deref;
use macros::get_langs;

get_langs!();

pub use langs::*;

use redis::Script;
use sea_orm::sea_query::OnConflict;
use sea_orm::{EntityTrait, IntoActiveModel};

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
    let counterkey = format!("ignc:{}", chat);

    let count: usize = REDIS
        .query(|mut q| async move {
            let count: usize = Script::new(
                r#"
                    local current
                    current = redis.call("incr",KEYS[1])
                    if current == 1 then
                        redis.call("expire",KEYS[1],ARGV[1])
                    end

                    if current == tonumber(ARGV[2]) then
                        redis.call("expire", KEYS[1], ARGV[3])
                    end
                    return current
                "#,
            )
            .key(&counterkey)
            .arg(CONFIG.timing.antifloodwait_time)
            .arg(CONFIG.timing.antifloodwait_count)
            .arg(CONFIG.timing.ignore_chat_time)
            .invoke_async(q.deref_mut())
            .await?;
            Ok(count)
        })
        .await?;
    Ok(count >= CONFIG.timing.antifloodwait_count)
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
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync;

    async fn speak_fmt<'a>(&self, messsage: CallSendMessage<'a>) -> Result<Option<Message>>;
    async fn reply_fmt<'a>(&self, messsage: CallSendMessage<'a>) -> Result<Option<Message>>;
    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync;
}

#[async_trait]
impl Speak for Message {
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            let md = MarkupBuilder::from_murkdown_chatuser(message, self.get_chatuser().as_ref())?;
            let (text, entities) = md.build();
            let m = TG
                .client()
                .build_send_message(self.get_chat().get_id(), text)
                .entities(&entities)
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn speak_fmt<'a>(&self, message: CallSendMessage<'a>) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            let m = message.build().await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn reply_fmt<'a>(&self, message: CallSendMessage<'a>) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            let m = message
                .reply_to_message_id(self.get_message_id())
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            let md = MarkupBuilder::from_murkdown_chatuser(message, self.get_chatuser().as_ref())?;
            let (text, entities) = md.build();
            let m = TG
                .client()
                .build_send_message(self.get_chat().get_id(), text)
                .entities(entities)
                .reply_to_message_id(self.get_message_id())
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl Speak for Chat {
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_id()).await? {
            let m = TG
                .client()
                .build_send_message(self.get_id(), message.as_ref())
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn speak_fmt<'a>(&self, message: CallSendMessage<'a>) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_id()).await? {
            let m = message.build().await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn reply_fmt<'a>(&self, message: CallSendMessage<'a>) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_id()).await? {
            let m = message.build().await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
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
    let c = dialogs::Model::from_chat(chat);
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
