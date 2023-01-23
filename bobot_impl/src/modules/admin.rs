use crate::{
    metadata::metadata,
    tg::{
        admin_helpers::{
            action_message, is_group_or_die, self_admin_or_die, GetCachedAdmins, IsAdmin,
        },
        command::{parse_cmd, Entities},
    },
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{Message, UpdateExt};

use futures::FutureExt;
use itertools::Itertools;
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

metadata!("Admin",
    { command = "admincache", help = "Refresh the cached list of admins" },
    { command = "admins", help = "Get a list of admins" },
    { command = "promote", help = "Promote a user to admin"},
    { command = "demote", help = "Demote a user" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn promote<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?; //TODO: handle granular permissions

    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    action_message(message, entities, |message, user| {
        async move {
            message.get_chat().promote(user.get_id()).await?;
            let name = user
                .get_username()
                .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));
            message.speak(rlformat!(lang, "promote", name)).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn demote<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?; //TODO: handle granular permissions

    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    action_message(message, entities, |message, user| {
        async move {
            message.get_chat().demote(user.get_id()).await?;
            let name = user
                .get_username()
                .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));
            message.speak(rlformat!(lang, "demote", name)).await?;

            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn listadmins(message: &Message) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let admins = message.get_chat().get_cached_admins().await?;
    let header = rlformat!(lang, "foundadmins", admins.len());
    let body = admins
        .values()
        .map(|v| {
            v.get_user()
                .get_username()
                .map(|u| u.into_owned())
                .unwrap_or_else(|| v.get_user().get_id().to_string())
        })
        .join("\n - ");
    message.speak(format!("{}:\n - {}", header, body)).await?;
    Ok(())
}

async fn admincache(message: &Message) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    message.get_chat().refresh_cached_admins().await?;
    message.speak(rlformat!(lang, "refreshac")).await?;
    Ok(())
}
pub async fn handle_update(update: &UpdateExt) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message).await?,
        _ => (),
    };
    Ok(())
}
async fn handle_command(message: &Message) -> Result<()> {
    if let Some((command, _, entities)) = parse_cmd(message) {
        log::info!("admin command {}", command);

        match command {
            "admincache" => admincache(message).await,
            "admins" => listadmins(message).await,
            "promote" => promote(message, &entities).await,
            "demote" => demote(message, &entities).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}
