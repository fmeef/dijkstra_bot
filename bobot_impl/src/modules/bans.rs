use crate::persist::admin::actions;
use crate::{
    metadata::metadata,
    statics::TG,
    tg::admin_helpers::*,
    tg::{
        command::{Command, Entities, TextArgs},
        dialog::dialog_or_default,
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

pub async fn unban_cmd<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?;
    action_message(message, entities, None, |message, user, _| {
        async move { unban(message, user).await }.boxed()
    })
    .await?;
    Ok(())
}

pub async fn ban_cmd<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?;
    action_message(message, entities, None, |message, user, _| {
        async move { ban(message, user).await }.boxed()
    })
    .await?;
    Ok(())
}

pub async fn mute_cmd<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(false)
        .set_can_send_media_messages(false)
        .set_can_send_polls(false)
        .set_can_send_other_messages(false)
        .build();
    update_actions_permissions(message, &permissions).await?;
    change_permissions_message(message, &entities, permissions).await?;
    message.speak(rlformat!(lang, "muteuser")).await?;
    Ok(())
}

pub async fn unmute_cmd<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(true)
        .set_can_send_media_messages(true)
        .set_can_send_polls(true)
        .set_can_send_other_messages(true)
        .build();
    update_actions_permissions(message, &permissions).await?;
    change_permissions_message(message, &entities, permissions).await?;
    message.speak(rlformat!(lang, "unmuteuser")).await?;
    Ok(())
}

pub async fn warn<'a>(
    message: &Message,
    entities: &Entities<'a>,
    args: &TextArgs<'a>,
) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message.get_from().admin_or_die(&message.get_chat()).await?; //TODO: handle granular permissions

    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    action_message(message, entities, Some(args), |message, user, args| {
        async move {
            let reason = args.map(|a| match a[0] {
                crate::tg::command::TextArg::Arg(a) => a.to_owned(),
                crate::tg::command::TextArg::Quote(q) => q.to_owned(),
            });

            let count = warn_user(message, user, reason.clone()).await?;
            // let action = get_action(message.get_chat_ref(), user).await?.map(|a| a.is_banned);

            let dialog = dialog_or_default(message.get_chat_ref()).await?;
            if count >= dialog.warn_limit {
                match dialog.action_type {
                    actions::ActionType::Mute => mute(message, user).await,
                    actions::ActionType::Ban => ban(message, user).await,
                    actions::ActionType::Shame => message.speak("shaming not implemented").await,
                }?;
            }

            let name = user
                .get_username()
                .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));

            if let Some(reason) = reason {
                message
                    .reply(format!(
                        "Yowzers! Warned user {} for {}, total warns: {}",
                        name, reason, count
                    ))
                    .await?;
            } else {
                message
                    .reply(format!(
                        "Yowzers! Warned user {}, total warns: {}",
                        name, count
                    ))
                    .await?;
            }
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn warns<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;

    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    action_message(message, entities, None, |message, user, _| {
        async move {
            let count = get_warns_count(message, user).await?;

            message.reply(format!("Warns: {}", count)).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
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

async fn handle_command<'a>(message: &Message, cmd: Option<&Command<'a>>) -> Result<()> {
    if let Some(&Command {
        cmd,
        ref entities,
        ref args,
    }) = cmd
    {
        log::info!("admin command {}", cmd);

        match cmd {
            "kickme" => kickme(message).await,
            "mute" => mute_cmd(message, &entities).await,
            "unmute" => unmute_cmd(message, &entities).await,
            "ban" => ban_cmd(message, &entities).await,
            "unban" => unban_cmd(message, &entities).await,
            "warn" => warn(message, &entities, args).await,
            "warns" => warns(message, &entities).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update<'a>(update: &UpdateExt, cmd: Option<&Command<'a>>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message, cmd).await?,
        _ => (),
    };
    Ok(())
}
