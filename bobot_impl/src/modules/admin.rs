use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    tg::{
        admin_helpers::GetCachedAdmins,
        command::{parse_cmd, Arg},
    },
};

metadata!("Admin",
    { command = "admincache", help = "Refresh the cached list of admins" },
    { command = "kickme", help = "Send a free course on termux hacking"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

#[allow(dead_code)]
async fn handle_command(message: &Message) -> BotResult<()> {
    if let Some(text) = message.get_text() {
        let (command, _) = parse_cmd(text)?;
        if let Arg::Arg(command) = command {
            log::info!("admin command {}", command);
            match command.as_str() {
                "/admincache" => {
                    message.get_chat().refresh_cached_admins().await?;
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
        _ => (),
    };
    Ok(())
}
