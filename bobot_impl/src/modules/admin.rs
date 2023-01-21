use botapi::{
    bot::BotResult,
    gen_types::{ChatPermissionsBuilder, Message, UpdateExt},
};
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::admin_helpers::mute_user_message,
    tg::{
        admin_helpers::{is_group_or_die, self_admin_or_die, GetCachedAdmins, IsAdmin},
        command::parse_cmd,
    },
    util::string::get_chat_lang,
};

metadata!("Admin",
    { command = "admincache", help = "Refresh the cached list of admins" },
    { command = "kickme", help = "Send a free course on termux hacking"},
    { command = "mute", help = "Mute a user"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn handle_command(message: &Message) -> BotResult<()> {
    if let Some((command, _, entities)) = parse_cmd(message) {
        log::info!("admin command {}", command);

        let lang = get_chat_lang(message.get_chat().get_id()).await?;
        match command {
            "admincache" => {
                message.get_chat().refresh_cached_admins().await?;
                TG.client()
                    .build_send_message(message.get_chat().get_id(), &rlformat!(lang, "refreshac"))
                    .build()
                    .await?;
            }
            "countadmins" => {
                let admins = message.get_chat().get_cached_admins().await?;
                TG.client()
                    .build_send_message(
                        message.get_chat().get_id(),
                        &rlformat!(lang, "foundadmins", admins.len()),
                    )
                    .build()
                    .await?;
            }
            "kickme" => {
                is_group_or_die(message.get_chat()).await?;
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
            "mute" => {
                mute_user_message(
                    message,
                    &entities,
                    &ChatPermissionsBuilder::new()
                        .set_can_send_messages(false)
                        .set_can_send_media_messages(false)
                        .set_can_send_polls(false)
                        .set_can_send_other_messages(false)
                        .build(),
                )
                .await?;

                TG.client()
                    .build_send_message(message.get_chat().get_id(), &rlformat!(lang, "muteuser"))
                    .build()
                    .await?;
            }
            "unmute" => {
                mute_user_message(
                    message,
                    &entities,
                    &ChatPermissionsBuilder::new()
                        .set_can_send_messages(true)
                        .set_can_send_media_messages(true)
                        .set_can_send_polls(true)
                        .set_can_send_other_messages(true)
                        .build(),
                )
                .await?;

                TG.client()
                    .build_send_message(message.get_chat().get_id(), &rlformat!(lang, "unmuteuser"))
                    .build()
                    .await?;
            }
            _ => (),
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
