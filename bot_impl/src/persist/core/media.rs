use std::borrow::Cow;

use crate::{
    statics::TG,
    tg::{
        admin_helpers::{is_dm, ChatUser, IntoChatUser},
        button::InlineKeyboardBuilder,
        command::{post_deep_link, Context},
        markdown::{button_deeplink_key, retro_fillings, MarkupBuilder},
    },
    util::{
        error::{BotError, Fail, Result},
        string::should_ignore_chat,
    },
};
use botapi::gen_types::{
    Chat, EReplyMarkup, FileData, InlineKeyboardButton, InputFile, InputMedia, InputMediaDocument,
    InputMediaPhoto, InputMediaVideo, Message, MessageEntity, User,
};
use futures::future::BoxFuture;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(
    EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Iden,
)]
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

pub struct SendMediaReply<'a, F>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
{
    context: &'a Context,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
    buttons: Option<InlineKeyboardBuilder>,
    extra_buttons: Option<Vec<InlineKeyboardButton>>,
    extra_entities: Option<Vec<MessageEntity>>,
    callback: Option<F>,
}

impl<'a, F> SendMediaReply<'a, F>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>> + Send + Sync,
{
    pub fn new(context: &'a Context, media_type: MediaType) -> Self {
        Self {
            context,
            media_type,
            text: None,
            media_id: None,
            buttons: None,
            extra_buttons: None,
            extra_entities: None,
            callback: None,
        }
    }

    pub fn button_callback(mut self, cb: F) -> Self {
        self.callback = Some(cb);
        self
    }

    pub fn media_id(mut self, media_id: Option<String>) -> Self {
        self.media_id = media_id;
        self
    }

    pub fn text(mut self, text: Option<String>) -> Self {
        self.text = text;
        self
    }

    async fn note_button(&mut self) -> Result<()> {
        if let Ok(message) = self.context.message() {
            let chatuser = message.get_chatuser();
            let is_dm = chatuser.as_ref().map(|v| is_dm(&v.chat)).unwrap_or(true);
            if self.buttons.is_none() {
                self.buttons = Some(InlineKeyboardBuilder::default());
            }
            let buttonlist = self
                .buttons
                .as_mut()
                .ok_or_else(|| self.context.fail_err("Missing buttons"))?;
            for l in buttonlist.get_mut() {
                for b in l.iter_mut() {
                    if let Some(ref button) = b.raw_text {
                        if button.starts_with("#") && button.len() > 1 && is_dm {
                            let tail = &button[1..];

                            b.button_url = None;
                            b.callback_data = Some(Uuid::new_v4().to_string());
                            self.callback
                                .as_ref()
                                .ok_or_else(|| self.context.fail_err("no callback"))?(
                                tail.to_owned(),
                                &b.clone().to_button(),
                            )
                            .await?;
                        } else if !is_dm && button.starts_with("#") && button.len() > 1 {
                            let chat = chatuser
                                .as_ref()
                                .ok_or_else(|| BotError::Generic("missing chatuser".to_owned()))?;
                            let chat = chat.chat.get_id();
                            let tail = &button[1..];

                            let url =
                                post_deep_link((chat, tail), |v| button_deeplink_key(v)).await?;
                            b.button_url = Some(url);
                        };
                    }
                }
            }
        }

        Ok(())
    }

    pub fn buttons(mut self, extra_buttons: Option<InlineKeyboardBuilder>) -> Self {
        self.buttons = extra_buttons;
        self
    }

    pub fn extra_buttons(mut self, extra_buttons: Option<Vec<InlineKeyboardButton>>) -> Self {
        self.extra_buttons = extra_buttons;
        self
    }

    pub fn extra_entities(mut self, extra_entities: Vec<MessageEntity>) -> Self {
        self.extra_entities = Some(extra_entities);
        self
    }

    pub async fn edit_media_reply_chatuser(mut self, current_message: &Message) -> Result<()> {
        if current_message.get_text().is_some() != (self.media_type == MediaType::Text) {
            TG.client
                .build_delete_message(
                    current_message.get_chat().get_id(),
                    current_message.get_message_id(),
                )
                .build()
                .await?;
            self.send_media_reply().await?;
        } else {
            self.note_button().await?;
            let text = self.text;
            let callback = self
                .callback
                .as_ref()
                .ok_or_else(|| self.context.fail_err("Need to set callback"))?;

            let chat = current_message.get_chat().get_id();
            if should_ignore_chat(chat).await? {
                return Ok(());
            }

            let text = text.unwrap_or_else(|| "".to_owned());
            let (text, entities, mut buttons) = if let Some(extra) = self.extra_entities {
                let mut buttons = self
                    .buttons
                    .unwrap_or_else(|| InlineKeyboardBuilder::default());
                let (text, entities) = retro_fillings(
                    text,
                    extra,
                    Some(&mut buttons),
                    &self
                        .context
                        .get_static()
                        .chatuser()
                        .ok_or_else(|| BotError::Generic("No chatuser".to_owned()))?,
                )
                .await?;
                log::info!("retro fillings: {}", text);
                (text, entities, buttons)
            } else {
                if let Ok(md) = MarkupBuilder::from_murkdown_button(
                    &text,
                    self.context.get_static().chatuser().as_ref(),
                    None,
                    &callback,
                    false,
                    false,
                )
                .await
                {
                    md.build_owned()
                } else {
                    (text, Vec::new(), InlineKeyboardBuilder::default())
                }
            };

            if let Some(extra_buttons) = self.extra_buttons {
                for button in extra_buttons {
                    buttons.button(button);
                }
            }

            let buttons = buttons.build();

            let input_media = match self.media_type {
                MediaType::Sticker => {
                    TG.client
                        .build_delete_message(chat, current_message.get_message_id())
                        .build()
                        .await?;
                    TG.client()
                        .build_send_sticker(
                            chat,
                            FileData::String(
                                self.media_id
                                    .ok_or_else(|| current_message.fail_err("invalid media"))?,
                            ),
                        )
                        .build()
                        .await?;
                    None
                }
                MediaType::Photo => Some(InputMedia::InputMediaPhoto(InputMediaPhoto::new(Some(
                    InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    ),
                )))),
                MediaType::Document => Some(InputMedia::InputMediaDocument(
                    InputMediaDocument::new(Some(InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    ))),
                )),
                MediaType::Video => Some(InputMedia::InputMediaVideo(InputMediaVideo::new(Some(
                    InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    ),
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
        }
        Ok(())
    }

    pub async fn send_media(mut self) -> Result<()> {
        self.note_button().await?;
        if let Some(chat) = self.context.chat() {
            let chat = chat.get_id();
            let callback = self
                .callback
                .ok_or_else(|| BotError::Generic("callback not set".to_owned()))?;
            let mut buttons = self
                .buttons
                .unwrap_or_else(|| InlineKeyboardBuilder::default());
            if should_ignore_chat(chat).await? {
                return Ok(());
            }

            let text = self.text.unwrap_or_else(|| "".to_owned());
            let (text, entities, mut buttons) = if let Some(extra) = self.extra_entities {
                let (text, entities) = retro_fillings(
                    text,
                    extra,
                    Some(&mut buttons),
                    &self
                        .context
                        .get_static()
                        .chatuser()
                        .ok_or_else(|| BotError::Generic("No chatuser".to_owned()))?,
                )
                .await?;
                log::info!("retro fillings: {}", text);
                (text, entities, buttons)
            } else {
                if let Ok(md) = MarkupBuilder::from_murkdown_button(
                    &text,
                    self.context.get_static().chatuser().as_ref(),
                    None,
                    &callback,
                    false,
                    false,
                )
                .await
                {
                    md.build_owned()
                } else {
                    (text, Vec::new(), InlineKeyboardBuilder::default())
                }
            };

            if let Some(extra_buttons) = self.extra_buttons {
                for button in extra_buttons {
                    buttons.button(button);
                }
            }

            let buttons = EReplyMarkup::InlineKeyboardMarkup(buttons.build());

            match self.media_type {
                MediaType::Sticker => {
                    TG.client()
                        .build_send_sticker(
                            chat,
                            FileData::String(
                                self.media_id
                                    .ok_or_else(|| self.context.fail_err("invalid media"))?,
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
                                self.media_id
                                    .ok_or_else(|| self.context.fail_err("invalid media"))?,
                            ),
                        )
                        .caption(&text)
                        .caption_entities(&entities)
                        .reply_markup(&buttons)
                        .build()
                        .await
                }
                MediaType::Document => {
                    TG.client()
                        .build_send_document(
                            chat,
                            FileData::String(
                                self.media_id
                                    .ok_or_else(|| self.context.fail_err("invalid media"))?,
                            ),
                        )
                        .reply_markup(&buttons)
                        .caption_entities(&entities)
                        .caption(&text)
                        .build()
                        .await
                }
                MediaType::Video => {
                    TG.client()
                        .build_send_video(
                            chat,
                            FileData::String(
                                self.media_id
                                    .ok_or_else(|| self.context.fail_err("invalid media"))?,
                            ),
                        )
                        .caption(&text)
                        .reply_markup(&buttons)
                        .caption_entities(&entities)
                        .build()
                        .await
                }
                MediaType::Text => {
                    TG.client()
                        .build_send_message(chat, &text)
                        .reply_markup(&buttons)
                        .entities(&entities)
                        .build()
                        .await
                }
            }?;
        }
        Ok(())
    }

    pub async fn send_media_reply(mut self) -> Result<()> {
        self.note_button().await?;
        let message = self.context.message()?;
        let chat = message.get_chat().get_id();
        let callback = self
            .callback
            .ok_or_else(|| BotError::Generic("callback not set".to_owned()))?;

        if should_ignore_chat(chat).await? {
            return Ok(());
        }

        let mut buttons = self
            .buttons
            .unwrap_or_else(|| InlineKeyboardBuilder::default());
        let text = self.text.unwrap_or_else(|| "".to_owned());
        let (text, entities, mut buttons) = if let Some(extra) = self.extra_entities {
            let (text, entities) = retro_fillings(
                text,
                extra,
                Some(&mut buttons),
                &self
                    .context
                    .get_static()
                    .chatuser()
                    .ok_or_else(|| BotError::Generic("No chatuser".to_owned()))?,
            )
            .await?;
            (text, entities, buttons)
        } else {
            if let Ok(md) = MarkupBuilder::from_murkdown_button(
                &text,
                self.context.get_static().chatuser().as_ref(),
                None,
                &callback,
                false,
                false,
            )
            .await
            {
                md.build_owned()
            } else {
                (text, Vec::new(), InlineKeyboardBuilder::default())
            }
        };

        if let Some(extra_buttons) = self.extra_buttons {
            for button in extra_buttons {
                buttons.button(button);
            }
        }

        let buttons = EReplyMarkup::InlineKeyboardMarkup(buttons.build());

        match self.media_type {
            MediaType::Sticker => {
                TG.client()
                    .build_send_sticker(
                        chat,
                        FileData::String(
                            self.media_id
                                .ok_or_else(|| self.context.fail_err("invalid media"))?,
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
                            self.media_id
                                .ok_or_else(|| message.fail_err("invalid media"))?,
                        ),
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
                        FileData::String(
                            self.media_id
                                .ok_or_else(|| message.fail_err("invalid media"))?,
                        ),
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
                        FileData::String(
                            self.media_id
                                .ok_or_else(|| message.fail_err("invalid media"))?,
                        ),
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
        MarkupBuilder::from_murkdown_button(&text, chatuser.as_ref(), None, &callback, false, false)
            .await
    {
        for ex in extra_buttons {
            md.buttons.button(ex);
        }
        md.build_owned()
    } else {
        (text, Vec::new(), InlineKeyboardBuilder::default())
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
                    buttons.build(),
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
        None,
        &callback,
        false,
        false,
    )
    .await
    {
        md.build_owned()
    } else {
        (text, Vec::new(), InlineKeyboardBuilder::default())
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
    let buttons = buttons.build();
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
    let (text, entities, buttons) = if let Some(extra) = extra_entities {
        let (text, entities) = retro_fillings(
            text,
            extra,
            None,
            &message
                .get_chatuser()
                .ok_or_else(|| BotError::Generic("No chatuser".to_owned()))?,
        )
        .await?;
        (text, entities, InlineKeyboardBuilder::default())
    } else {
        if let Ok(md) = MarkupBuilder::from_murkdown_button(
            &text,
            message.get_chatuser().as_ref(),
            None,
            &callback,
            false,
            false,
        )
        .await
        {
            md.build_owned()
        } else {
            (text, Vec::new(), InlineKeyboardBuilder::default())
        }
    };
    let buttons = buttons.build();
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
