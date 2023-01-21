use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::{command::parse_cmd, markdown::MarkupBuilder},
    util::string::{should_ignore_chat, Lang, Speak},
};

metadata!("Piracy detection",
    { command = "report", help = "Report a pirate for termination" },
    { command = "crash", help = "Intentionally trigger a floodwait for debugging"},
    { command = "markdown", help = "Reply to a message to parse as markdown"},
    { command = "murkdown", help = "Reply to a message to parse as murkdown" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn handle_murkdown(message: &Message) -> BotResult<bool> {
    if let Some(message) = message.get_reply_to_message() {
        if let Some(text) = message.get_text() {
            match MarkupBuilder::from_murkdown(text) {
                Ok(md) => {
                    if !should_ignore_chat(message.get_chat().get_id()).await? {
                        let (msg, entities) = md.build();
                        TG.client()
                            .build_send_message(message.get_chat().get_id(), msg)
                            .entities(entities)
                            .build()
                            .await?;
                    }
                }

                Err(err) => {
                    message.speak(rlformat!(Lang::En, "test", err)).await?;
                }
            }
        }
    }
    Ok(false)
}
async fn handle_markdown(message: &Message) -> BotResult<bool> {
    if let Some(message) = message.get_reply_to_message() {
        if let Some(text) = message.get_text() {
            let md = MarkupBuilder::from_markdown(text);
            let (msg, entities) = md.build();
            TG.client()
                .build_send_message(message.get_chat().get_id(), msg)
                .entities(entities)
                .build()
                .await?;
        }
    }
    Ok(false)
}

#[allow(dead_code)]
async fn handle_command(message: &Message) -> BotResult<()> {
    if should_ignore_chat(message.get_chat().get_id()).await? {
        return Ok(());
    }
    if let Some((command, _, _)) = parse_cmd(message) {
        log::info!("piracy command {}", command);
        match command {
            "crash" => TG.client().close().await?,
            "markdown" => handle_markdown(message).await?,
            "murkdown" => handle_murkdown(message).await?,
            _ => false,
        };
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
