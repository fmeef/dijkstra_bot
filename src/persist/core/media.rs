use crate::{
    persist::core::entity::DefaultAction,
    statics::TG,
    tg::{
        admin_helpers::{is_dm, IntoChatUser},
        button::InlineKeyboardBuilder,
        command::{post_deep_link, Context},
        markdown::{button_deeplink_key, retro_fillings, EntityMessage, MarkupBuilder},
    },
    util::{
        error::{BotError, Fail, Result},
        string::should_ignore_chat,
    },
};
use botapi::gen_types::{
    EReplyMarkup, FileData, InlineKeyboardButton, InputFile, InputMedia, InputMediaAudioBuilder,
    InputMediaDocumentBuilder, InputMediaPhotoBuilder, InputMediaVideoBuilder,
    LinkPreviewOptionsBuilder, Message, MessageEntity, ReplyParametersBuilder,
};
use futures::future::BoxFuture;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

pub trait GetMediaId {
    fn get_media_id(&self) -> Option<(&'_ str, MediaType)>;
}

impl GetMediaId for Message {
    fn get_media_id(&self) -> Option<(&'_ str, MediaType)> {
        if let Some(image) = self.get_photo().and_then(|p| p.first()) {
            return Some((image.get_file_id(), MediaType::Photo));
        }

        if let Some(document) = self.get_document() {
            return Some((document.get_file_id(), MediaType::Document));
        }

        if let Some(sticker) = self.get_sticker() {
            return Some((sticker.get_file_id(), MediaType::Sticker));
        }

        if let Some(video) = self.get_video() {
            return Some((video.get_file_id(), MediaType::Video));
        }

        None
    }
}

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
    #[sea_orm(num_value = 6)]
    Audio,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sticker => f.write_str("sticker"),
            Self::Photo => f.write_str("photo"),
            Self::Document => f.write_str("document"),
            Self::Text => f.write_str("text"),
            Self::Video => f.write_str("video"),
            Self::Audio => f.write_str("audio"),
        }
    }
}

impl MediaType {
    /// Rose bot uses different media ids than we do, provide translation for import/export
    pub fn get_rose_type(&self) -> i64 {
        match self {
            Self::Sticker => 1,
            Self::Photo => 2,
            Self::Document => 8,
            Self::Video => 3,
            Self::Text => 0,
            Self::Audio => 6,
        }
    }

    /// Rose bot uses different media ids than we do, provide translation for import/export
    pub fn from_rose_type(t: i64) -> Self {
        match t {
            1 => Self::Sticker,
            2 => Self::Photo,
            8 => Self::Document,
            3 => Self::Video,
            _ => Self::Text,
        }
    }
}

/// Returns a tuple containing the MediaType and caption if exists for the provided message
pub fn get_media_type(message: &Message) -> Result<(Option<String>, MediaType)> {
    if let Some(photo) = message
        .get_photo()
        .and_then(|p| p.first().map(|v| v.to_owned()))
    {
        Ok((Some(photo.get_file_id().to_owned()), MediaType::Photo))
    } else if let Some(sticker) = message.get_sticker().map(|s| s.get_file_id().to_owned()) {
        Ok((Some(sticker), MediaType::Sticker))
    } else if let Some(document) = message.get_document().map(|d| d.get_file_id().to_owned()) {
        Ok((Some(document), MediaType::Document))
    } else if let Some(video) = message.get_video().map(|v| v.get_file_id().to_owned()) {
        Ok((Some(video), MediaType::Video))
    } else if let Some(audio) = message.get_audio().map(|v| v.get_file_id().to_owned()) {
        Ok((Some(audio), MediaType::Audio))
    } else if message.get_text().is_some() {
        Ok((None, MediaType::Text))
    } else {
        message.fail("invalid")
    }
}

/// Helper type for sending media referenced from database with optional InlineKeyboardMarkup
// and formatted captions
pub struct SendMediaReply<'a, F>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>>
        + Send
        + Sync
        + 'static,
{
    context: &'a Context,
    media_type: MediaType,
    text: Option<String>,
    media_id: Option<String>,
    buttons: Option<InlineKeyboardBuilder>,
    override_buttons: Option<InlineKeyboardBuilder>,
    extra_entities: Option<Vec<MessageEntity>>,
    actions: Option<Vec<DefaultAction>>,
    callback: Option<F>,
}

impl<'a, F> SendMediaReply<'a, F>
where
    F: for<'b> Fn(String, &'b InlineKeyboardButton) -> BoxFuture<'b, Result<()>>
        + Send
        + Sync
        + 'static,
{
    pub fn new(context: &'a Context, media_type: MediaType) -> Self {
        Self {
            context,
            media_type,
            text: None,
            media_id: None,
            buttons: None,
            override_buttons: None,
            extra_entities: None,
            callback: None,
            actions: None,
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

    pub fn actions(mut self, actions: Option<Vec<DefaultAction>>) -> Self {
        self.actions = actions;
        self
    }

    pub async fn entity_message_nofail(mut self, message: EntityMessage) -> Self {
        let (text, entities, kb) = message.builder.build_murkdown_nofail().await;
        self.extra_entities = Some(entities);
        self.text = Some(text);
        self.override_buttons = Some(kb);
        self
    }

    pub async fn entity_message(mut self, message: EntityMessage) -> Result<Self> {
        let (text, entities, kb) = message.builder.build_murkdown_nofail().await;
        self.extra_entities = Some(entities);
        self.text = Some(text);
        self.override_buttons = Some(kb);
        Ok(self)
    }

    async fn note_button(&mut self) -> Result<()> {
        if let Ok(message) = self.context.message() {
            let chatuser = message.get_chatuser();
            let is_dm = chatuser.as_ref().map(|v| is_dm(v.chat)).unwrap_or(true);
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
                        if button.starts_with('#') && button.len() > 1 && is_dm {
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
                        } else if !is_dm && button.starts_with('#') && button.len() > 1 {
                            let chat = chatuser
                                .as_ref()
                                .ok_or_else(|| BotError::Generic("missing chatuser".to_owned()))?;
                            let chat = chat.chat.get_id();
                            let tail = &button[1..];

                            let url = post_deep_link((chat, tail), button_deeplink_key).await?;
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

    pub fn override_buttons(mut self, extra_buttons: Option<InlineKeyboardBuilder>) -> Self {
        self.override_buttons = extra_buttons;
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
                .ok_or_else(|| self.context.fail_err("Need to set callback"))?;

            let chat = current_message.get_chat().get_id();
            if should_ignore_chat(chat).await? {
                return Ok(());
            }

            let text = text.unwrap_or_else(|| "".to_owned());
            let buttons = self.buttons.unwrap_or_default();
            let (text, entities, buttons) = if let Some(extra) = self.extra_entities {
                let (text, extra, mut buttons) = if self.actions.is_some() {
                    let (text, mut entities, buttons) = MarkupBuilder::new(None)
                        .set_text(text)
                        .filling(false)
                        .header(false)
                        .input_actions(self.actions)
                        .callback(callback)
                        .chatuser(self.context.get_static().chatuser().as_ref())
                        .build_murkdown_nofail()
                        .await;
                    entities.extend_from_slice(extra.as_slice());
                    (text, entities, buttons)
                } else {
                    (text, extra, buttons)
                };
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
                MarkupBuilder::new(None)
                    .set_text(text)
                    .filling(false)
                    .header(false)
                    .input_actions(self.actions)
                    .callback(callback)
                    .chatuser(self.context.get_static().chatuser().as_ref())
                    .build_murkdown_nofail()
                    .await
            };

            let buttons = if let Some(extra_buttons) = self.override_buttons {
                extra_buttons.build()
            } else {
                buttons.build()
            };

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
                MediaType::Photo => Some(InputMedia::InputMediaPhoto(
                    InputMediaPhotoBuilder::new(Some(InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    )))
                    .set_caption(text)
                    .set_caption_entities(entities)
                    .build(),
                )),
                MediaType::Document => Some(InputMedia::InputMediaDocument(
                    InputMediaDocumentBuilder::new(Some(InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    )))
                    .set_caption(text)
                    .set_caption_entities(entities)
                    .build(),
                )),
                MediaType::Video => Some(InputMedia::InputMediaVideo(
                    InputMediaVideoBuilder::new(Some(InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    )))
                    .set_caption(text)
                    .set_caption_entities(entities)
                    .build(),
                )),
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
                MediaType::Audio => Some(InputMedia::InputMediaAudio(
                    InputMediaAudioBuilder::new(Some(InputFile::String(
                        self.media_id
                            .ok_or_else(|| current_message.fail_err("invalid media"))?,
                    )))
                    .set_caption(text)
                    .set_caption_entities(entities)
                    .build(),
                )),
            };

            if let Some(input_media) = input_media {
                TG.client
                    .build_edit_message_media(&input_media)
                    .media(&input_media)
                    .message_id(current_message.get_message_id())
                    .chat_id(current_message.get_chat().get_id())
                    .reply_markup(&buttons)
                    .build()
                    .await?;

                // TG.client
                //     .build_edit_message_caption()
                //     .message_id(current_message.get_message_id())
                //     .chat_id(current_message.get_chat().get_id())
                //     .caption(&text)
                //     .caption_entities(&entities)
                //     .reply_markup(&buttons)
                //     .build()
                //     .await?;
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
            let buttons = self.buttons.unwrap_or_default();
            if should_ignore_chat(chat).await? {
                return Ok(());
            }

            let text = self.text.unwrap_or_else(|| "".to_owned());
            let (text, entities, mut buttons) = if let Some(extra) = self.extra_entities {
                let (text, extra, mut buttons) = if self.actions.is_some() {
                    let (text, mut entities, buttons) = MarkupBuilder::new(None)
                        .set_text(text)
                        .filling(false)
                        .header(false)
                        .input_actions(self.actions)
                        .callback(callback)
                        .chatuser(self.context.get_static().chatuser().as_ref())
                        .build_murkdown_nofail()
                        .await;
                    entities.extend_from_slice(extra.as_slice());
                    (text, entities, buttons)
                } else {
                    (text, extra, buttons)
                };
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
                MarkupBuilder::new(None)
                    .set_text(text)
                    .filling(false)
                    .header(false)
                    .input_actions(self.actions)
                    .callback(callback)
                    .chatuser(self.context.get_static().chatuser().as_ref())
                    .build_murkdown_nofail()
                    .await
            };

            if let Some(extra_buttons) = self.override_buttons {
                buttons = extra_buttons;
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
                MediaType::Audio => {
                    TG.client
                        .build_send_audio(
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
                        .link_preview_options(
                            &LinkPreviewOptionsBuilder::new()
                                .set_is_disabled(true)
                                .build(),
                        )
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

        let buttons = self.buttons.unwrap_or_default();
        let text = self.text.unwrap_or_else(|| "".to_owned());
        let (text, entities, mut buttons) = if let Some(extra) = self.extra_entities {
            let (text, extra, mut buttons) = if self.actions.is_some() {
                let (text, mut entities, buttons) = MarkupBuilder::new(None)
                    .set_text(text)
                    .filling(false)
                    .header(false)
                    .input_actions(self.actions)
                    .callback(callback)
                    .chatuser(self.context.get_static().chatuser().as_ref())
                    .build_murkdown_nofail()
                    .await;
                entities.extend_from_slice(extra.as_slice());
                (text, entities, buttons)
            } else {
                (text, extra, buttons)
            };
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
            MarkupBuilder::new(None)
                .set_text(text)
                .filling(false)
                .header(false)
                .input_actions(self.actions)
                .callback(callback)
                .chatuser(self.context.get_static().chatuser().as_ref())
                .build_murkdown_nofail()
                .await
        };

        if let Some(extra_buttons) = self.override_buttons {
            buttons = extra_buttons;
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
                    .reply_parameters(
                        &ReplyParametersBuilder::new(message.get_message_id()).build(),
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
                                .ok_or_else(|| message.fail_err("invalid media"))?,
                        ),
                    )
                    .caption(&text)
                    .caption_entities(&entities)
                    .reply_markup(&buttons)
                    .reply_parameters(
                        &ReplyParametersBuilder::new(message.get_message_id()).build(),
                    )
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
                    .reply_parameters(
                        &ReplyParametersBuilder::new(message.get_message_id()).build(),
                    )
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
                    .reply_parameters(
                        &ReplyParametersBuilder::new(message.get_message_id()).build(),
                    )
                    .caption(&text)
                    .reply_markup(&buttons)
                    .caption_entities(&entities)
                    .build()
                    .await
            }

            MediaType::Audio => {
                TG.client
                    .build_send_audio(
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
                    .reply_parameters(
                        &ReplyParametersBuilder::new(message.get_message_id()).build(),
                    )
                    .reply_markup(&buttons)
                    .entities(&entities)
                    .link_preview_options(
                        &LinkPreviewOptionsBuilder::new()
                            .set_is_disabled(true)
                            .build(),
                    )
                    .build()
                    .await
            }
        }?;

        Ok(())
    }
}
