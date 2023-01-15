use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::{
        admin_helpers::{self_admin_or_die, GetCachedAdmins, IsAdmin},
        command::{parse_cmd, Arg},
    },
    util::string::get_chat_lang,
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

            let lang = get_chat_lang(message.get_chat().get_id()).await?;
            match command.as_str() {
                "/admincache" => {
                    message.get_chat().refresh_cached_admins().await?;
                    TG.client()
                        .build_send_message(
                            message.get_chat().get_id(),
                            &rlformat!(lang, "refreshac"),
                        )
                        .build()
                        .await?;
                }
                "/countadmins" => {
                    let admins = message.get_chat().get_cached_admins().await?;
                    TG.client()
                        .build_send_message(
                            message.get_chat().get_id(),
                            &rlformat!(lang, "foundadmins", admins.len()),
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
                                &rlformat!(lang, "kickadmin"),
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
