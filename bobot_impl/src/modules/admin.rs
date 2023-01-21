use botapi::{
    bot::BotResult,
    gen_types::{Message, UpdateExt},
};
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

use crate::{
    metadata::metadata,
    statics::TG,
    tg::user::get_user_username,
    tg::{
        admin_helpers::{is_group_or_die, self_admin_or_die, GetCachedAdmins, IsAdmin},
        command::{parse_cmd, EntityArg},
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
                is_group_or_die(message.get_chat()).await?;
                self_admin_or_die(message.get_chat()).await?;
                message.get_from().admin_or_die(message.get_chat()).await?;
                let lang = get_chat_lang(message.get_chat().get_id()).await?;
                match entities.front() {
                    Some(EntityArg::Mention(name)) => {
                        if let Some(user) = get_user_username(name).await? {
                            TG.client()
                                .build_send_message(
                                    message.get_chat().get_id(),
                                    &format!("found user {}", user.get_id()),
                                )
                                .build()
                                .await?;
                        } else {
                            TG.client()
                                .build_send_message(
                                    message.get_chat().get_id(),
                                    &rlformat!(lang, "usernotfound"),
                                )
                                .build()
                                .await?;
                        }
                    }
                    Some(EntityArg::TextMention(user)) => {
                        TG.client()
                            .build_send_message(
                                message.get_chat().get_id(),
                                &format!("found user {}", user.get_id()),
                            )
                            .build()
                            .await?;
                    }
                    _ => {
                        TG.client()
                            .build_send_message(
                                message.get_chat().get_id(),
                                &rlformat!(lang, "specifyuser"),
                            )
                            .build()
                            .await?;
                    }
                };
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
