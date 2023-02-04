use crate::persist::admin::actions;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::{
    metadata::metadata,
    tg::admin_helpers::*,
    tg::{
        command::{Command, Entities, TextArgs},
        dialog::dialog_or_default,
    },
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{Message, UpdateExt, User};
use futures::FutureExt;
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

metadata!("Warns",
    { command = "warn", help = "Warns a user"},
    { command = "warns", help = "Get warn count of a user"},
    { command = "clearwarns", help = "Delete all warns for a user"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn warn_ban(message: &Message, user: &User, count: i32) -> Result<()> {
    ban(message, user).await?;
    Ok(())
}

async fn warn_mute(message: &Message, user: &User, count: i32) -> Result<()> {
    mute(message, user).await?;
    Ok(())
}

async fn warn_shame(message: &Message, user: &User, count: i32) -> Result<()> {
    message.speak("shaming not implemented").await?;
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
            if user.is_admin(message.get_chat_ref()).await? {
                return Err(BotError::speak(
                    "I am not going to warn an admin!",
                    message.get_chat().get_id(),
                ));
            }
            let reason = args
                .map(|a| if a.args.len() > 0 { Some(a.text) } else { None })
                .flatten();

            let count = warn_user(message, user, reason.map(|v| v.to_owned())).await?;
            // let action = get_action(message.get_chat_ref(), user).await?.map(|a| a.is_banned);

            let dialog = dialog_or_default(message.get_chat_ref()).await?;
            if count >= dialog.warn_limit {
                match dialog.action_type {
                    actions::ActionType::Mute => warn_mute(message, user, count).await,
                    actions::ActionType::Ban => warn_ban(message, user, count).await,
                    actions::ActionType::Shame => warn_shame(message, user, count).await,
                }?;
            }

            let name = user.name_humanreadable();
            if let Some(reason) = reason {
                message
                    .reply(format!(
                        "Yowzers! Warned user {} for \"{}\", total warns: {}",
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

pub async fn clear<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;
    action_message(message, entities, None, |message, user, _| {
        async move {
            clear_warns(message.get_chat_ref(), user).await?;

            let name = user
                .get_username()
                .unwrap_or_else(|| std::borrow::Cow::Owned(user.get_id().to_string()));
            message
                .reply(format!("Cleared warns for user {}", name))
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;

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
            "warn" => warn(message, &entities, args).await,
            "warns" => warns(message, &entities).await,
            "clearwarns" => clear(message, &entities).await,
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
