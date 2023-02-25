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
use botapi::gen_types::{Message, UpdateExt};
use chrono::Duration;
use futures::FutureExt;
use humantime::format_duration;
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

metadata!("Warns",
    { command = "warn", help = "Warns a user"},
    { command = "warns", help = "Get warn count of a user"},
    { command = "clearwarns", help = "Delete all warns for a user"},
    { command = "warntime", help = "Sets time before warns expire. Usage: /warntime 6m for 6 minutes"},
    { command = "warnmode", help = "Set the action when max warns are reached. Can be 'mute', 'ban' or 'shame'"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}
pub async fn warn<'a>(
    message: &Message,
    entities: &Entities<'a>,
    args: &TextArgs<'a>,
) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    action_message(message, entities, Some(args), |message, user, args| {
        async move {
            if user.is_admin(message.get_chat_ref()).await? {
                return Err(BotError::speak(
                    &rlformat!(lang, "warnadmin"),
                    message.get_chat().get_id(),
                ));
            }

            let reason = args
                .map(|a| {
                    if a.args.len() > 0 {
                        Some(a.text.trim())
                    } else {
                        None
                    }
                })
                .flatten();

            let dialog = dialog_or_default(message.get_chat_ref()).await?;
            let time = dialog.warn_time.map(|t| Duration::seconds(t));
            let count = warn_user(message, user, reason.map(|v| v.to_owned()), &time).await?;

            if count >= dialog.warn_limit {
                match dialog.action_type {
                    actions::ActionType::Mute => warn_mute(message, user, count).await,
                    actions::ActionType::Ban => warn_ban(message, user, count).await,
                    actions::ActionType::Shame => warn_shame(message, user, count).await,
                    actions::ActionType::Warn => Ok(()),
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
            let warns = get_warns(message, user).await?;
            let list = warns
                .into_iter()
                .map(|w| {
                    format!(
                        "Reason: {}",
                        w.reason.unwrap_or_else(|| rlformat!(lang, "noreason"))
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");
            message
                .reply(rlformat!(lang, "warns", user.name_humanreadable(), list))
                .await?;
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

async fn set_time<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;
    if let Some(time) = parse_duration(&Some(args.as_slice()), message.get_chat().get_id())? {
        set_warn_time(message.get_chat_ref(), time.num_seconds()).await?;
        let time = format_duration(time.to_std()?);
        message.reply(format!("Set warn time to {}", time)).await?;
    } else {
        message.reply("Specify a time").await?;
    }
    Ok(())
}

async fn cmd_warn_mode<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .get_from()
        .admin_or_die(message.get_chat_ref())
        .await?;
    set_warn_mode(message.get_chat_ref(), args.text).await?;
    message
        .reply(format!("Set warn mode {}", args.text))
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
            "warntime" => set_time(message, args).await,
            "warnmode" => cmd_warn_mode(message, args).await,
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
