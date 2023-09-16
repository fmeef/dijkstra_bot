use std::borrow::Cow;

use crate::{
    statics::TG,
    tg::{
        admin_helpers::{ChatUser, IntoChatUser},
        markdown::MarkupBuilder,
    },
    util::{
        error::{Fail, Result},
        string::should_ignore_chat,
    },
};
use botapi::gen_types::{
    Chat, EReplyMarkup, FileData, InlineKeyboardButton, InlineKeyboardMarkup, InputFile,
    InputMedia, InputMediaDocument, InputMediaPhoto, InputMediaVideo, Message, MessageEntity, User,
};
use futures::future::BoxFuture;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug, Iden)]
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
        message.fail("invalid")
    }
}

pub async fn send_media_reply_chatuser<F>(
    current_chat: &Chat,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
    user: Option<&User>,
    extra_buttons: Vec<InlineKeyboardButton>,
    callback: F,
) -> Result<()>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
{
    let chat = current_chat.get_id();
    if should_ignore_chat(chat).await? {
        return Ok(());
    }

    let text = text.unwrap_or_else(|| "".to_owned());

    let chatuser = user.map(|v| ChatUser {
        chat: Cow::Borrowed(current_chat),
        user: Cow::Borrowed(v),
    });
    let (text, entities, buttons) = if let Ok(mut md) =
        MarkupBuilder::from_murkdown_button(&text, chatuser.as_ref(), &callback).await
    {
        for ex in extra_buttons {
            md.buttons.button(ex);
        }
        md.build_owned()
    } else {
        (text, Vec::new(), InlineKeyboardMarkup::default())
    };
    match media_type {
        MediaType::Sticker => {
            TG.client()
                .build_send_sticker(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| current_chat.fail_err("invalid media"))?,
                    ),
                )
                .build()
                .await
        }
        MediaType::Photo => {
            TG.client()
                .build_send_photo(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| current_chat.fail_err("invalid media"))?,
                    ),
                )
                .build()
                .await
        }
        MediaType::Document => {
            TG.client()
                .build_send_document(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| current_chat.fail_err("invalid media"))?,
                    ),
                )
                .build()
                .await
        }
        MediaType::Video => {
            TG.client()
                .build_send_video(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| current_chat.fail_err("invalid media"))?,
                    ),
                )
                .build()
                .await
        }
        MediaType::Text => {
            TG.client()
                .build_send_message(chat, &text)
                .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                    buttons,
                ))
                .entities(&entities)
                .build()
                .await
        }
    }?;
    Ok(())
}

pub async fn edit_media_reply_chatuser<F>(
    current_message: &Message,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
    callback: F,
) -> Result<()>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
{
    let chat = current_message.get_chat().get_id();
    if should_ignore_chat(chat).await? {
        return Ok(());
    }

    let text = text.unwrap_or_else(|| "".to_owned());
    let (text, entities, buttons) = if let Ok(md) = MarkupBuilder::from_murkdown_button(
        &text,
        current_message.get_chatuser().as_ref(),
        &callback,
    )
    .await
    {
        md.build_owned()
    } else {
        (text, Vec::new(), InlineKeyboardMarkup::default())
    };

    if current_message.get_text().is_some() != (media_type == MediaType::Text) {
        TG.client
            .build_delete_message(
                current_message.get_chat().get_id(),
                current_message.get_message_id(),
            )
            .build()
            .await?;
        return send_media_reply_chatuser(
            current_message.get_chat_ref(),
            media_type,
            Some(text),
            media_id,
            current_message.get_from_ref(),
            vec![],
            callback,
        )
        .await;
    }

    let input_media = match media_type {
        MediaType::Sticker => {
            TG.client
                .build_delete_message(chat, current_message.get_message_id())
                .build()
                .await?;
            TG.client()
                .build_send_sticker(
                    chat,
                    FileData::String(
                        media_id.ok_or_else(|| current_message.fail_err("invalid media"))?,
                    ),
                )
                .build()
                .await?;
            None
        }
        MediaType::Photo => Some(InputMedia::InputMediaPhoto(InputMediaPhoto::new(Some(
            InputFile::String(media_id.ok_or_else(|| current_message.fail_err("invalid media"))?),
        )))),
        MediaType::Document => Some(InputMedia::InputMediaDocument(InputMediaDocument::new(
            Some(InputFile::String(
                media_id.ok_or_else(|| current_message.fail_err("invalid media"))?,
            )),
        ))),
        MediaType::Video => Some(InputMedia::InputMediaVideo(InputMediaVideo::new(Some(
            InputFile::String(media_id.ok_or_else(|| current_message.fail_err("invalid media"))?),
        )))),
        MediaType::Text => {
            TG.client
                .build_edit_message_text(&text)
                .message_id(current_message.get_message_id())
                .chat_id(current_message.get_chat().get_id())
                .entities(&entities)
                .reply_markup(&buttons)
                .build()
                .await?;
            None
        }
    };

    if let Some(input_media) = input_media {
        TG.client
            .build_edit_message_media(&input_media)
            .message_id(current_message.get_message_id())
            .chat_id(current_message.get_chat().get_id())
            .build()
            .await?;

        TG.client
            .build_edit_message_caption()
            .message_id(current_message.get_message_id())
            .chat_id(current_message.get_chat().get_id())
            .caption(&text)
            .caption_entities(&entities)
            .reply_markup(&buttons)
            .build()
            .await?;
    }
    Ok(())
}
pub async fn send_media_reply<F>(
    message: &Message,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
    extra_entities: Option<Vec<MessageEntity>>,
    callback: F,
) -> Result<()>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
{
    let chat = message.get_chat().get_id();
    if should_ignore_chat(chat).await? {
        return Ok(());
    }

    let text = text.unwrap_or_else(|| "".to_owned());
    let (text, mut entities, buttons) = if let Ok(md) =
        MarkupBuilder::from_murkdown_button(&text, message.get_chatuser().as_ref(), &callback).await
    {
        md.build_owned()
    } else {
        (text, Vec::new(), InlineKeyboardMarkup::default())
    };

    if let Some(extra) = extra_entities {
        entities.extend_from_slice(extra.as_slice());
    }

    let buttons = EReplyMarkup::InlineKeyboardMarkup(buttons);

    match media_type {
        MediaType::Sticker => {
            TG.client()
                .build_send_sticker(
                    chat,
                    FileData::String(media_id.ok_or_else(|| message.fail_err("invalid media"))?),
                )
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Photo => {
            TG.client()
                .build_send_photo(
                    chat,
                    FileData::String(media_id.ok_or_else(|| message.fail_err("invalid media"))?),
                )
                .caption(&text)
                .caption_entities(&entities)
                .reply_markup(&buttons)
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Document => {
            TG.client()
                .build_send_document(
                    chat,
                    FileData::String(media_id.ok_or_else(|| message.fail_err("invalid media"))?),
                )
                .reply_markup(&buttons)
                .caption_entities(&entities)
                .caption(&text)
                .reply_to_message_id(message.get_message_id())
                .build()
                .await
        }
        MediaType::Video => {
            TG.client()
                .build_send_video(
                    chat,
                    FileData::String(media_id.ok_or_else(|| message.fail_err("invalid media"))?),
                )
                .reply_to_message_id(message.get_message_id())
                .caption(&text)
                .reply_markup(&buttons)
                .caption_entities(&entities)
                .build()
                .await
        }
        MediaType::Text => {
            TG.client()
                .build_send_message(chat, &text)
                .reply_to_message_id(message.get_message_id())
                .reply_markup(&buttons)
                .entities(&entities)
                .build()
                .await
        }
    }?;
    Ok(())
}
