use crate::{
    statics::TG,
    tg::markdown::MarkupBuilder,
    util::{
        error::{BotError, Result},
        string::should_ignore_chat,
    },
};
use botapi::gen_types::{FileData, Message};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum MediaType {
    #[sea_orm(num_value = 1)]
    Sticker,
    #[sea_orm(num_value = 2)]
    Photo,
    #[sea_orm(num_value = 3)]
    Document,
    #[sea_orm(num_value = 4)]
    Text,
    #[sea_orm(num_value = 5)]
    Video,
}

pub fn get_media_type<'a>(message: &'a Message) -> Result<(Option<String>, MediaType)> {
    if let Some(photo) = message
        .get_photo()
        .map(|p| p.first().map(|v| v.to_owned()))
        .flatten()
    {
        Ok((Some(photo.get_file_id().into_owned()), MediaType::Photo))
    } else if let Some(sticker) = message.get_sticker().map(|s| s.get_file_id().into_owned()) {
        Ok((Some(sticker), MediaType::Sticker))
    } else if let Some(document) = message.get_document().map(|d| d.get_file_id().into_owned()) {
        Ok((Some(document), MediaType::Document))
    } else if let Some(video) = message.get_video().map(|v| v.get_file_id().into_owned()) {
        Ok((Some(video), MediaType::Video))
    } else if let Some(_) = message.get_text() {
        Ok((None, MediaType::Text))
    } else {
        Err(BotError::speak("invalid", message.get_chat().get_id()))
    }
}

pub async fn send_media_reply(
    message: &Message,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
) -> Result<()> {
    let chat = message.get_chat().get_id();
    if should_ignore_chat(chat).await? {
        return Ok(());
    }
    match media_type {
        MediaType::Sticker => {
            TG.client()
                .build_send_sticker(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| BotError::speak("invalid media", chat))?,
                    ),
                )
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Photo => {
            TG.client()
                .build_send_photo(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| BotError::speak("invalid media", chat))?,
                    ),
                )
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Document => {
            TG.client()
                .build_send_document(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| BotError::speak("invalid media", chat))?,
                    ),
                )
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Video => {
            TG.client()
                .build_send_video(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| BotError::speak("invalid media", chat))?,
                    ),
                )
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Text => {
            let text = text.ok_or_else(|| BotError::speak("invalid text", chat))?;
            let (text, entities) = if let Ok(md) = MarkupBuilder::from_murkdown(&text) {
                md.build_owned()
            } else {
                (text, Vec::new())
            };
            TG.client()
                .build_send_message(chat, &text)
                .reply_to_message_id(message.get_message_id())
                .entities(&entities)
                .build()
                .await
        }
    }?;
    Ok(())
}
