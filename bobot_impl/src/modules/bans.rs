use crate::{
    metadata::metadata,
    statics::TG,
    tg::admin_helpers::change_permissions_message,
    tg::{
        admin_helpers::{action_message, is_group_or_die, self_admin_or_die, IsAdmin},
        command::{parse_cmd, Entities},
    },
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{ChatPermissionsBuilder, Message, UpdateExt};
use futures::FutureExt;
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

metadata!("Bans",
    { command = "kickme", help = "Send a free course on termux hacking"},
    { command = "mute", help = "Mute a user"},
    { command = "unmute", help = "Unmute a user"},
    { command = "ban", help = "Bans a user"},
    { command = "unban", help = "Unbans a user"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn mute<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    change_permissions_message(
        message,
        &entities,
        ChatPermissionsBuilder::new()
            .set_can_send_messages(false)
            .set_can_send_media_messages(false)
            .set_can_send_polls(false)
            .set_can_send_other_messages(false)
            .build(),
    )
    .await?;
    message.speak(rlformat!(lang, "muteuser")).await?;
    Ok(())
}

async fn kickme(message: &Message) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    if message.get_from().is_admin(&message.get_chat()).await? {
        message.speak(rlformat!(lang, "kickadmin")).await?;
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
            message.speak(rlformat!(lang, "kickme")).await?;
        }
    }

    Ok(())
}

async fn ban<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?;
    action_message(message, entities, |message, user| {
        async move {
            if let Some(senderchat) = message.get_sender_chat() {
                TG.client()
                    .build_ban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
                    .build()
                    .await?;
                let name = senderchat
                    .get_username()
                    .unwrap_or_else(|| std::borrow::Cow::Owned(senderchat.get_id().to_string()));
                message.speak(rlformat!(lang, "banchat", name)).await?;
            } else {
                TG.client()
                    .build_ban_chat_member(message.get_chat().get_id(), user.get_id())
                    .build()
                    .await?;

                let name = user
                    .get_username()
                    .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));
                message.speak(rlformat!(lang, "banned", name)).await?;
            }
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn unban<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?;
    action_message(message, entities, |message, user| {
        async move {
            if let Some(senderchat) = message.get_sender_chat() {
                TG.client()
                    .build_unban_chat_sender_chat(message.get_chat().get_id(), senderchat.get_id())
                    .build()
                    .await?;
                let name = senderchat
                    .get_username()
                    .unwrap_or_else(|| std::borrow::Cow::Owned(senderchat.get_id().to_string()));
                message.speak(rlformat!(lang, "unbanchat", name)).await?;
            } else {
                TG.client()
                    .build_unban_chat_member(message.get_chat().get_id(), user.get_id())
                    .build()
                    .await?;

                let name = user
                    .get_username()
                    .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));
                message.speak(rlformat!(lang, "unbanned", name)).await?;
            }
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn unmute<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    change_permissions_message(
        message,
        &entities,
        ChatPermissionsBuilder::new()
            .set_can_send_messages(true)
            .set_can_send_media_messages(true)
            .set_can_send_polls(true)
            .set_can_send_other_messages(true)
            .build(),
    )
    .await?;
    message.speak(rlformat!(lang, "unmuteuser")).await?;
    Ok(())
}

async fn handle_command(message: &Message) -> Result<()> {
    if let Some((command, _, entities)) = parse_cmd(message) {
        log::info!("admin command {}", command);

        match command {
            "kickme" => kickme(message).await,
            "mute" => mute(message, &entities).await,
            "unmute" => unmute(message, &entities).await,
            "ban" => ban(message, &entities).await,
            "unban" => unban(message, &entities).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update(update: &UpdateExt) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message).await?,
        _ => (),
    };
    Ok(())
}
