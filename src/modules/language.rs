use crate::statics::TG;
use crate::tg::command::{Cmd, Context};

use crate::tg::user::{GetChat, RecordChat};
use crate::util::error::BotError;
use crate::util::string::{set_chat_lang, should_ignore_chat, Lang, Speak};
use crate::{
    metadata::metadata,
    tg::dialog::{Conversation, ConversationState},
    util::error::Result,
    util::string::get_langs,
};

use botapi::gen_types::Message;
use macros::{inline_lang, lang_fmt, update_handler};
use uuid::Uuid;

metadata! {
    "Language",
    r#"This bot supports automatic translations! Set the language for the current chat
    using this module
    "#,
    { command = "setlang", help = "Set languge" }
}

inline_lang! {
    { "en" => r#"testfmef: "thingy""# }
}

async fn handle_terminal_state(
    current: Uuid,
    conv: Conversation,
    chat: i64,
    reply: i64,
) -> Result<()> {
    let chat = chat
        .get_chat()
        .await?
        .ok_or_else(|| BotError::speak("Chat not found", chat, Some(reply)))?;
    if let Some(state) = conv.get_state(&current) {
        let lang = Lang::from_code(&state.content);

        log::info!("set chat lang to {:?}", state.content);
        match lang {
            Lang::Invalid => {
                chat.reply(lang_fmt!(lang, "invalidlang")).await?;
            }
            l => {
                set_chat_lang(&chat, l).await?;
                chat.reply(lang_fmt!(l, "setlang")).await?;
            }
        }
    } else {
        log::info!("setlang with invalid state");
    }
    Ok(())
}

async fn get_lang_conversation(message: &Message, current: &Lang) -> Result<Conversation> {
    let mut state = ConversationState::new_prefix(
        "setlang".to_owned(),
        lang_fmt!(current, "currentlang"),
        message.get_chat().get_id(),
        message.get_from().map(|u| u.get_id()).ok_or_else(|| {
            BotError::speak(
                "user is not a user... what",
                message.get_chat().get_id(),
                Some(message.message_id),
            )
        })?,
        "button",
    )?;

    let start = state.get_start()?.state_id;
    get_langs().iter().for_each(|lang| {
        // log::warn!("supported lang: {:?}", lang);
        let success = state.add_state(lang.into_code());
        state.add_transition(start, success, lang.into_code(), lang.into_code());
    });
    message.get_chat().record_chat().await?;
    let id = message.get_chat().get_id();
    let message_id = message.message_id;
    state.state_callback(move |uuid, conv| {
        log::info!("conversation state {}", uuid);
        if uuid != start {
            tokio::spawn(async move {
                if let Err(err) = handle_terminal_state(uuid, conv, id, message_id).await {
                    log::warn!("terminal state error {}", err);
                    err.record_stats();
                }
            });
        }
    });

    let state = state.build();
    state.write_self().await?;
    Ok(state)
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd: "setlang",
        message,
        lang,
        ..
    }) = ctx.cmd()
    {
        let conv = get_lang_conversation(message, lang).await?;

        if should_ignore_chat(message.get_chat().get_id()).await? {
            return Ok(());
        }
        TG.client()
            .build_send_message(
                message.get_chat().get_id(),
                &conv.get_current().await?.content,
            )
            .reply_markup(&botapi::gen_types::EReplyMarkup::InlineKeyboardMarkup(
                conv.get_current_markup(3).await?,
            ))
            .build()
            .await?;
    }
    Ok(())
}

#[update_handler]
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}
