//! Provides apis for managing strings, localization, and sending messages to chats
//! All message sending should be done through this api for both localization/translation
//! and ratelimiting to work

pub use crate::langs::*;
use crate::persist::core::dialogs;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisStr};
use crate::statics::{CHAT_GOVERNER, CONFIG, DB, REDIS, TG};
use crate::tg::admin_helpers::IntoChatUser;
use crate::tg::markdown::{EntityMessage, MarkupBuilder, MessageOrFile};
use crate::util::error::{BotError, BoxedBotError, Result};
use async_trait::async_trait;
use botapi::bot::Part;
use botapi::gen_types::{
    Chat, EReplyMarkup, FileData, LinkPreviewOptionsBuilder, Message, ReplyParametersBuilder,
};
use chrono::Duration;
use redis::Script;
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::Set;
use sea_orm::{EntityTrait, IntoActiveModel};
use std::ops::DerefMut;

/// Returns false if ratelimiting is triggered. This function should be called before
/// every attempt to send a messsage in a chat, as calling it determines ratelimiting
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

    CHAT_GOVERNER.until_key_ready(&chat).await;
    Ok(count >= CONFIG.timing.antifloodwait_count)
}

/// Sets a redis key that causes all official methods of sending messages to suspend
/// as long as the key exists. Part of ratelimiting system
pub async fn ignore_chat(chat: i64, time: &Duration) -> Result<()> {
    let key = format!("ign:{}", chat);
    let _: () = REDIS
        .pipe(|q| q.set(&key, true).expire(&key, time.num_seconds()))
        .await?;
    Ok(())
}

/// Extension trait with fuctions for sending messages. Types that implement this trait should be
/// types containing distinct references to chats or objects that can be replied to.
#[async_trait]
pub trait Speak {
    /// Send a text message to the chat associated with this type. Murkdown is parsed if valid
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync;

    /// Sends a telegram api send_message builder, potentially with existing MessageEntities or
    /// other formatting
    async fn speak_fmt(&self, messsage: EntityMessage) -> Result<Option<Message>>;

    /// Replies with a telegram api send_message builder, potentially with existing MessageEntities or
    /// other formatting
    async fn reply_fmt(&self, messsage: EntityMessage) -> Result<Option<Message>>;

    /// Replies with a text message to the chat associated with this type. Murkdown is parsed if valid
    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync;

    /// Replies with a text message to the chat associated with this type. Murkdown is parsed if valid
    async fn force_reply<T>(&self, message: T, reply: i64) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync;
}

#[async_trait]
impl Speak for i64 {
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(*self).await? {
            if message.as_ref().len() > 4096 {
                let bytes = FileData::Part(
                    Part::text(message.as_ref().to_owned()).file_name("message.txt"),
                );
                let message = TG.client.build_send_document(*self, bytes).build().await?;
                return Ok(Some(message));
            }

            let (text, entities, markup) = MarkupBuilder::new(None)
                .set_text(message.as_ref().to_owned())
                .filling(true)
                .header(false)
                .build_murkdown_nofail()
                .await;

            for entity in entities.iter() {
                if entity.offset + entity.length > text.len() as i64 {
                    log::error!("entity {entity:?} overflowed");
                    return Err(BoxedBotError::from(BotError::Generic(format!(
                        "entity {entity:?} overflowed"
                    ))));
                }
            }

            let m = TG
                .client()
                .build_send_message(*self, &text)
                .entities(&entities)
                .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(markup.build()))
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;

            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn speak_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(*self).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => file.build().await?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn reply_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(*self).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => file.build().await?,
            }))
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

    async fn force_reply<T>(&self, message: T, reply: i64) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(*self).await? {
            if message.as_ref().len() > 4096 {
                let bytes = FileData::Part(
                    Part::text(message.as_ref().to_owned()).file_name("message.txt"),
                );
                let message = TG.client.build_send_document(*self, bytes).build().await?;
                return Ok(Some(message));
            }

            let (text, entities, markup) = MarkupBuilder::new(None)
                .set_text(message.as_ref().to_owned())
                .filling(true)
                .header(false)
                .build_murkdown_nofail()
                .await;

            let m = TG
                .client()
                .build_send_message(*self, &text)
                .entities(&entities)
                .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(markup.build()))
                .reply_parameters(&ReplyParametersBuilder::new(reply).build())
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;

            Ok(Some(m))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl Speak for Message {
    async fn speak<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            if message.as_ref().len() > 4096 {
                let bytes = FileData::Part(
                    Part::text(message.as_ref().to_owned()).file_name("message.txt"),
                );
                let message = TG
                    .client
                    .build_send_document(self.get_chat().get_id(), bytes)
                    .build()
                    .await?;
                return Ok(Some(message));
            }

            let (text, entities, markup) = MarkupBuilder::new(None)
                .set_text(message.as_ref().to_owned())
                .filling(true)
                .header(false)
                .chatuser(self.get_chatuser().as_ref())
                .build_murkdown_nofail()
                .await;

            let m = TG
                .client()
                .build_send_message(self.get_chat().get_id(), &text)
                .entities(&entities)
                .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(markup.build()))
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;

            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn speak_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => file.build().await?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn reply_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .reply_parameters(&ReplyParametersBuilder::new(self.message_id).build())
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => {
                    file.reply_parameters(&ReplyParametersBuilder::new(self.message_id).build())
                        .build()
                        .await?
                }
            }))
        } else {
            Ok(None)
        }
    }

    async fn reply<T>(&self, message: T) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            if message.as_ref().len() > 4096 {
                let bytes = FileData::Part(
                    Part::text(message.as_ref().to_owned()).file_name("message.txt"),
                );

                let message = TG
                    .client
                    .build_send_document(self.get_chat().get_id(), bytes)
                    .reply_parameters(&ReplyParametersBuilder::new(self.get_message_id()).build())
                    .build()
                    .await?;
                return Ok(Some(message));
            }

            let (text, entities, markup) = MarkupBuilder::new(None)
                .set_text(message.as_ref().to_owned())
                .filling(true)
                .header(false)
                .chatuser(self.get_chatuser().as_ref())
                .build_murkdown_nofail()
                .await;

            let m = TG
                .client()
                .build_send_message(self.get_chat().get_id(), &text)
                .entities(&entities)
                .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(markup.build()))
                .reply_parameters(&ReplyParametersBuilder::new(self.get_message_id()).build())
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn force_reply<T>(&self, message: T, reply: i64) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_chat().get_id()).await? {
            if message.as_ref().len() > 4096 {
                let bytes = FileData::Part(
                    Part::text(message.as_ref().to_owned()).file_name("message.txt"),
                );

                let message = TG
                    .client
                    .build_send_document(self.get_chat().get_id(), bytes)
                    .reply_parameters(&ReplyParametersBuilder::new(self.get_message_id()).build())
                    .build()
                    .await?;
                return Ok(Some(message));
            }

            let (text, entities, markup) = MarkupBuilder::new(None)
                .set_text(message.as_ref().to_owned())
                .filling(true)
                .header(false)
                .chatuser(self.get_chatuser().as_ref())
                .build_murkdown_nofail()
                .await;

            let m = TG
                .client()
                .build_send_message(self.get_chat().get_id(), &text)
                .entities(&entities)
                .reply_markup(&EReplyMarkup::InlineKeyboardMarkup(markup.build()))
                .reply_parameters(&ReplyParametersBuilder::new(reply).build())
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
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
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn speak_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_id()).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => file.build().await?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn reply_fmt(&self, mut message: EntityMessage) -> Result<Option<Message>> {
        if !should_ignore_chat(self.get_id()).await? {
            Ok(Some(match message.call().await {
                MessageOrFile::Message(message) => {
                    message
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
                        .build()
                        .await?
                }
                MessageOrFile::File(file) => file.build().await?,
            }))
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
    async fn force_reply<T>(&self, message: T, reply: i64) -> Result<Option<Message>>
    where
        T: AsRef<str> + Send + Sync,
    {
        if !should_ignore_chat(self.get_id()).await? {
            let m = TG
                .client()
                .build_send_message(self.get_id(), message.as_ref())
                .reply_parameters(&ReplyParametersBuilder::new(reply).build())
                .link_preview_options(
                    &LinkPreviewOptionsBuilder::new()
                        .set_is_disabled(true)
                        .build(),
                )
                .build()
                .await?;
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }
}

fn get_lang_key(chat: i64) -> String {
    format!("lang:{}", chat)
}

/// Gets the language config for the current chat
pub async fn get_chat_lang(chat: i64) -> Result<Lang> {
    let key = get_lang_key(chat);
    let res = default_cache_query(
        |_, _| async move {
            Ok(Some(
                dialogs::Entity::find_by_id(chat)
                    .one(*DB)
                    .await?
                    .map(|v| v.language)
                    .unwrap_or_else(|| Lang::En),
            ))
        },
        Duration::try_hours(12).unwrap(),
    )
    .query(&key, &())
    .await?;
    Ok(res.unwrap_or(Lang::En))
}

/// Sets the current langauge config for the chat
pub async fn set_chat_lang(chat: &Chat, lang: Lang) -> Result<()> {
    let r = RedisStr::new(&lang)?;
    let mut c = dialogs::Model::from_chat(chat).await?;
    c.language = Set(lang);
    let key = get_lang_key(chat.get_id());
    let _: () = REDIS
        .pipe(|p| {
            p.set(&key, r)
                .expire(&key, Duration::try_hours(12).unwrap().num_seconds())
        })
        .await?;
    dialogs::Entity::insert(c.into_active_model())
        .on_conflict(
            OnConflict::column(dialogs::Column::ChatId)
                .update_column(dialogs::Column::Language)
                .to_owned(),
        )
        .exec(*DB)
        .await?;

    Ok(())
}

pub trait AlignCharBoundry {
    fn align_char_boundry(&self, idx: usize) -> usize;
}

impl AlignCharBoundry for &str {
    fn align_char_boundry(&self, mut idx: usize) -> usize {
        while !self.is_char_boundary(idx) && idx < self.len() {
            idx += 1;
        }
        idx
    }
}

impl AlignCharBoundry for String {
    fn align_char_boundry(&self, idx: usize) -> usize {
        self.as_str().align_char_boundry(idx)
    }
}

#[cfg(test)]
mod test {
    use super::AlignCharBoundry;

    #[test]
    fn align_cyrillic_shit() {
        let coin_scam = "выносим с игры быстро если успеете https://playdog.io";

        for x in 0..coin_scam.len() {
            let align = coin_scam.align_char_boundry(x);
            println!("x={} align={} len={}", x, align, coin_scam.len());

            assert!(coin_scam.is_char_boundary(align));
        }
    }
}
