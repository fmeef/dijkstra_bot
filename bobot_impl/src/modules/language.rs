use crate::statics::TG;
use crate::tg::user::{GetChat, RecordChat};
use crate::util::string::{set_chat_lang, Lang};
use crate::{
    metadata::metadata,
    tg::{
        command::parse_cmd,
        dialog::{Conversation, ConversationState},
    },
    util::string::{get_chat_lang, get_langs},
};
use anyhow::{anyhow, Result};

use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use macros::{inline_lang, rlformat};
use sea_orm_migration::MigrationTrait;
use uuid::Uuid;

metadata! {
    "Language and localization",
    { command = "setlang", help = "Set languge" }
}

inline_lang! {
    { "en" => r#"testfmef: "thingy""# }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn handle_terminal_state(current: Uuid, conv: Conversation, chat: i64) -> BotResult<()> {
    let chat = chat
        .get_chat()
        .await?
        .ok_or_else(|| anyhow!("missing chat"))?;
    if let Some(state) = conv.get_state(&current) {
        let lang = Lang::from_code(&state.content);
        match lang {
            Lang::Invalid => {
                TG.client()
                    .build_send_message(chat.get_id(), &rlformat!(lang, "invalidlang"))
                    .build()
                    .await?;
            }
            l => {
                set_chat_lang(&chat, l).await?;
                TG.client()
                    .build_send_message(chat.get_id(), &rlformat!(l, "setlang"))
                    .build()
                    .await?;
            }
        }
    }
    Ok(())
}

async fn get_lang_conversation(message: &Message) -> Result<Conversation> {
    let current = get_chat_lang(message.get_chat().get_id()).await?;
    let mut state = ConversationState::new_prefix(
        "setlang".to_owned(),
        rlformat!(current, "currentlang"),
        message.get_chat().get_id(),
        message
            .get_from()
            .map(|u| u.get_id())
            .ok_or_else(|| anyhow!("not user"))?,
        "button",
    )?;

    let start = state.get_start()?.state_id;
    get_langs().iter().for_each(|lang| {
        let success = state.add_state(rlformat!(lang, "setlang"));
        state.add_transition(start, success, lang.into_code());
    });
    message.get_chat().record_chat().await?;
    let id = message.get_chat().get_id();
    state.state_callback(move |uuid, conv| {
        if uuid != start {
            tokio::spawn(async move {
                if let Err(err) = handle_terminal_state(uuid, conv, id).await {
                    if let Some(error) = err.get_response() {
                        if let Some(error_code) = error.error_code {
                            crate::statics::count_error_code(error_code);
                        }
                    }
                }
            });
        }
    });

    let state = state.build();
    state.write_self().await?;
    Ok(state)
}

async fn handle_command(message: &Message) -> BotResult<()> {
    if let Some(text) = message.get_text() {
        if let Some((command, _)) = parse_cmd(text) {
            log::info!("language command {}", command);
            match command {
                "setlang" => {
                    let conv = get_lang_conversation(message).await?;
                    TG.client()
                        .build_send_message(
                            message.get_chat().get_id(),
                            &conv.get_current().await?.content,
                        )
                        .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                            conv.get_current_markup().await?,
                        ))
                        .build()
                        .await?;
                }
                _ => (),
            };
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_update(update: &UpdateExt) -> BotResult<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message).await?,
        _ => (),
    };
    Ok(())
}
