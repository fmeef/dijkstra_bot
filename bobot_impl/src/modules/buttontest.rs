use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::command::{parse_cmd, Arg},
};

metadata!("Piracy detection",
   { command = "report", help = "Report a pirate for termination" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

#[allow(dead_code)]
async fn handle_command(message: &Message) -> BotResult<()> {
    if let Some(text) = message.get_text() {
        let (command, _) = parse_cmd(text)?;
        if let Arg::Arg(command) = command {
            log::info!("piracy command {}", command);
            match command.as_str() {
                "/crash" => TG.client().close().await?,
                _ => false,
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
