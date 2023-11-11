use crate::metadata::{metadata, ModuleHelpers};
use crate::persist::redis::RedisCache;
use crate::statics::{DB, REDIS, TG};

use crate::tg::admin_helpers::IntoChatUser;
use crate::tg::button::{InlineKeyboardBuilder, OnPush};
use crate::tg::command::{
    get_content, handle_deep_link, Cmd, Context, InputType, TextArg, TextArgs,
};

use crate::tg::markdown::{button_deeplink_key, MarkupBuilder};
use crate::tg::notes::{get_hash_key, get_note_by_name, handle_transition, refresh_notes};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::rosemd::RoseMdDecompiler;
use crate::tg::user::Username;
use crate::util::error::{BotError, Fail, Result};
use crate::util::string::Speak;
use ::sea_orm_migration::prelude::*;
use botapi::gen_types::{Message, MessageEntity};
use futures::FutureExt;
use lazy_static::__Deref;
use macros::lang_fmt;
use redis::AsyncCommands;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use crate::persist::core::{entity, media::*, notes};

metadata!("Notes",
    r#"
    Easily store and retrive text, media, and other content by keywords. 
    Useful for storing answers to often asked questions or searching uploaded media.     
    "#,
    Helper,
    { sub = "Examples", content = r#"teset"# },
    { command = "save", help = "Saves a note" },
    { command = "get", help = "Get a note" },
    { command = "delete", help = "Delete a note" },
    { command = "notes", help = "List all notes for the current chat"}
);

#[derive(Serialize, Deserialize)]
struct ExportNotes {
    notes: Vec<NotesItem>,
    private_notes: bool,
}

#[derive(Serialize, Deserialize)]
struct NotesItem {
    data_id: String,
    name: String,
    text: String,
    #[serde(rename = "type")]
    note_type: i64,
}

struct Helper;

#[async_trait::async_trait]
impl ModuleHelpers for Helper {
    async fn export(&self, chat: i64) -> Result<Option<serde_json::Value>> {
        let notes = refresh_notes(chat).await?;
        let items: Vec<NotesItem> = notes
            .into_iter()
            .map(|(note, (model, entities, buttons))| {
                let buttons = if let Some(buttons) = buttons {
                    buttons
                } else {
                    InlineKeyboardBuilder::default()
                }
                .build();
                let text = if let Some(text) = &model.text {
                    text
                } else {
                    ""
                };
                let text =
                    RoseMdDecompiler::new(text, &entities, buttons.get_inline_keyboard_ref())
                        .decompile()
                        .replace("\n", "\\n");
                NotesItem {
                    data_id: model.media_id.unwrap_or_else(|| String::new()),
                    name: note,
                    text,
                    note_type: model.media_type.get_rose_type(),
                }
            })
            .collect();

        let out = ExportNotes {
            private_notes: false,
            notes: items,
        };

        Ok(Some(serde_json::to_value(&out)?))
    }

    async fn import(&self, _: i64, _: serde_json::Value) -> Result<()> {
        Ok(())
    }

    fn supports_export(&self) -> Option<&'static str> {
        Some("notes")
    }

    fn get_migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        vec![]
    }
}

async fn get_model<'a>(message: &'a Message, args: &'a TextArgs<'a>) -> Result<notes::Model> {
    let input_type = get_content(message, args)?;
    let res = match input_type {
        InputType::Reply(name, text, message) => {
            let chatuser = message.get_chatuser();
            let (media_id, media_type) = get_media_type(message)?;
            let text = text
                .map(|t| Some(t))
                .unwrap_or_else(|| message.get_caption_ref());
            let (text, entity_id) = if let Some(text) = text {
                let extra = message.get_entities().map(|v| v.into_owned());

                let md = MarkupBuilder::new(extra)
                    .chatuser(chatuser.as_ref())
                    .filling(false)
                    .header(false)
                    .set_text(text.to_owned());
                let (text, entities, buttons) = md.build_murkdown().await?;
                let entity_id = entity::insert(DB.deref(), &entities, buttons).await?;
                (Some(text), Some(entity_id))
            } else {
                (None, None)
            };
            notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text,
                media_id,
                media_type,
                protect: false,
                entity_id,
            }
        }

        InputType::Command(name, content, message) => {
            let (media_id, media_type) = get_media_type(message)?;
            let chatuser = message.get_chatuser();
            let content = content
                .map(|t| Some(t))
                .unwrap_or_else(|| message.get_caption_ref());

            let (text, entity_id) = if let Some(text) = content {
                log::info!("content {}", text);

                let extra = message.get_entities().map(|v| v.into_owned());

                let md = MarkupBuilder::new(extra)
                    .chatuser(chatuser.as_ref())
                    .filling(false)
                    .header(false)
                    .set_text(text.to_owned());
                let (text, entities, buttons) = md.build_murkdown().await?;
                let entity_id = entity::insert(DB.deref(), &entities, buttons).await?;
                (Some(text), Some(entity_id))
            } else {
                (None, None)
            };
            notes::Model {
                name: (*name).to_owned(),
                chat: message.get_chat().get_id(),
                text,
                media_id,
                media_type,
                protect: false,
                entity_id,
            }
        }
    };

    Ok(res)
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "save" => save(ctx, &args).await,
            "get" => get(ctx).await,
            "delete" => delete(message, args).await,
            "notes" => list_notes(ctx).await,
            "start" => {
                let note: Option<(i64, String)> =
                    handle_deep_link(ctx, |k| button_deeplink_key(k)).await?;
                if let Some((chat, note)) = note {
                    log::info!("handling note deep link {} {}", chat, note);
                    print_chat(ctx, note, chat).await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }?;
    }
    Ok(())
}

async fn print_note(
    ctx: &Context,
    note: notes::Model,
    entities: Vec<MessageEntity>,
    buttons: Option<InlineKeyboardBuilder>,
    note_chat: i64,
) -> Result<()> {
    let c = ctx.clone();
    SendMediaReply::new(ctx, note.media_type)
        .button_callback(move |note, button| {
            let c = c.clone();
            async move {
                button.on_push(move |b| async move {
                    TG.client
                        .build_answer_callback_query(b.get_id_ref())
                        .build()
                        .await?;
                    handle_transition(&c, note_chat, note, b).await?;
                    Ok(())
                });

                Ok(())
            }
            .boxed()
        })
        .text(note.text)
        .media_id(note.media_id)
        .extra_entities(entities)
        .buttons(buttons)
        .send_media_reply()
        .await?;
    Ok(())
}

async fn print(message: &Context, name: String) -> Result<()> {
    print_chat(message, name, message.message()?.get_chat().get_id()).await
}

async fn print_chat(ctx: &Context, name: String, chat: i64) -> Result<()> {
    if let Some((note, entities, buttons)) = get_note_by_name(name, chat).await? {
        if let Some(buttons) = buttons.as_ref() {
            log::info!("note buttons {:?}", buttons.get());
        }
        print_note(ctx, note, entities, buttons, chat).await?;
        Ok(())
    } else {
        ctx.fail("Note not found")
    }
}

async fn get<'a>(ctx: &Context) -> Result<()> {
    ctx.is_group_or_die().await?;
    let message = ctx.message()?;
    if let Some(&Cmd { ref args, .. }) = ctx.cmd() {
        let name = match args.args.first() {
            Some(TextArg::Arg(name)) => Some(name),
            Some(TextArg::Quote(name)) => Some(name),
            _ => None,
        };
        if let Some(name) = name {
            print(ctx, (*name).to_owned()).await
        } else {
            Err(BotError::speak(
                "missing note name, try again weenie",
                message.get_chat().get_id(),
            ))
        }
    } else {
        Err(BotError::Generic("not a command".to_owned()))
    }
}

async fn delete<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let model = get_model(message, args).await?;
    let name = model.name.clone();
    let hash_key = get_hash_key(message.get_chat().get_id());
    REDIS.sq(|q| q.hdel(&hash_key, &model.name)).await?;
    notes::Entity::delete_by_id((model.name, message.get_chat().get_id()))
        .exec(DB.deref())
        .await?;
    message.speak(format!("Deleted note {}", name)).await?;
    Ok(())
}

async fn list_notes(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    let notes = refresh_notes(message.get_chat().get_id()).await?;
    let m = [lang_fmt!(
        ctx,
        "listnotes",
        message.get_chat().name_humanreadable()
    )]
    .into_iter()
    .chain(notes.iter().map(|(n, _)| format!("- {}", n)))
    .collect::<Vec<String>>()
    .join("\n");
    message.reply(m).await?;
    Ok(())
}

async fn save<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
    let chat = message.get_chat().name_humanreadable();
    let model = get_model(message, args).await?;
    let key = format!("note:{}:{}", message.get_chat().get_id(), model.name);
    log::info!("save key: {}", key);
    let hash_key = get_hash_key(message.get_chat().get_id());
    REDIS.sq(|q| q.del(&hash_key)).await?;
    let name = model.name.clone();
    notes::Entity::insert(model.cache(key).await?)
        .on_conflict(
            OnConflict::columns([notes::Column::Name, notes::Column::Chat])
                .update_columns([
                    notes::Column::Text,
                    notes::Column::MediaId,
                    notes::Column::MediaType,
                    notes::Column::Protect,
                    notes::Column::EntityId,
                ])
                .to_owned(),
        )
        .exec(DB.deref())
        .await?;

    message
        .speak(lang_fmt!(ctx, "savednote", name, chat))
        .await?;
    Ok(())
}

pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    if let Ok(message) = cmd.message() {
        if let Some(text) = message.get_text_ref() {
            if text.starts_with("#") && text.len() > 1 {
                let tail = &text[1..];
                print(cmd, tail.to_owned()).await?;
            }
        }
    }
    handle_command(cmd).await?;

    Ok(())
}
