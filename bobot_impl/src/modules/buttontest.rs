use crate::tg::admin_helpers::IntoChatUser;
use crate::tg::command::Context;
use crate::util::error::Result;
use crate::{
    metadata::metadata,
    statics::TG,
    tg::markdown::MarkupBuilder,
    util::string::{should_ignore_chat, Lang, Speak},
};
use botapi::gen_types::{Message, UpdateExt};
use macros::lang_fmt;
use sea_orm_migration::MigrationTrait;

metadata!("Antipiracy",
    r#"
    This is just a debugging module, it will be removed eventually. 
    "#,
    { command = "report", help = "Report a pirate for termination" },
    { command = "crash", help = "Intentionally trigger a floodwait for debugging"},
    { command = "markdown", help = "Reply to a message to parse as markdown"},
    { command = "murkdown", help = "Reply to a message to parse as murkdown" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn handle_murkdown(message: &Message) -> Result<bool> {
    if let Some(message) = message.get_reply_to_message() {
        if let Some(text) = message.get_text() {
            match MarkupBuilder::from_murkdown_chatuser(text, message.get_chatuser().as_ref()) {
                Ok(md) => {
                    if !should_ignore_chat(message.get_chat().get_id()).await? {
                        if should_ignore_chat(message.get_chat().get_id()).await? {
                            return Ok(false);
                        }
                        let (msg, entities) = md.build();

                        TG.client()
                            .build_send_message(message.get_chat().get_id(), msg)
                            .entities(entities)
                            .build()
                            .await?;
                    }
                }

                Err(err) => {
                    message.speak(lang_fmt!(Lang::En, "test", err)).await?;
                }
            }
        }
    }
    Ok(false)
}
async fn handle_markdown(message: &Message) -> Result<bool> {
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
async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, _, _, message)) = ctx.cmd() {
        log::info!("piracy command {}", cmd);
        match cmd {
            //            "crash" => TG.client().close().await?,
            "crash" => {
                message
                    .reply("Eh eh ehhh... You didn't say the magic word!")
                    .await?;
            }
            "markdown" => {
                handle_markdown(message).await?;
            }
            "murkdown" => {
                handle_murkdown(message).await?;
            }
            _ => (),
        };
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Context<'a>) -> Result<()> {
    handle_command(cmd).await
}
