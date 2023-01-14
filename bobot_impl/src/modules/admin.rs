use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::{
        admin_helpers::{self_admin_or_die, GetCachedAdmins, IsAdmin},
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

async fn handle_command(message: &Message) -> BotResult<()> {
    if let Some(text) = message.get_text() {
        let (command, _) = parse_cmd(text)?;
        if let Arg::Arg(command) = command {
            log::info!("admin command {}", command);
            match command.as_str() {
                "/admincache" => {
                    message.get_chat().refresh_cached_admins().await?;
                    TG.client()
                        .build_send_message(
                            message.get_chat().get_id(),
                            "Successfully refreshed admin cache",
                        )
                        .build()
                        .await?;
                }
                "/countadmins" => {
                    let admins = message.get_chat().get_cached_admins().await?;
                    TG.client()
                        .build_send_message(
                            message.get_chat().get_id(),
                            &format!("Found {} admins", admins.len()),
                        )
                        .build()
                        .await?;
                }
                "/kickme" => {
                    self_admin_or_die(message.get_chat()).await?;
                    if message.get_from().is_admin(message.get_chat()).await? {
                        TG.client()
                            .build_send_message(
                                message.get_chat().get_id(),
                                "I'm not going to kick an admin",
                            )
                            .build()
                            .await?;
                    } else {
                        if let Some(from) = message.get_from() {
                            TG.client()
                                .build_ban_chat_member(message.get_chat().get_id(), from.get_id())
                                .build()
                                .await?;
                            TG.client()
                                .build_unban_chat_member(message.get_chat().get_id(), from.get_id())
                                .build()
                                .await?;
                        }
                    }
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
