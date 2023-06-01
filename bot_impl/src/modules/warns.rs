use crate::tg::command::Context;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::util::string::Lang;
use crate::{
    metadata::metadata,
    tg::admin_helpers::*,
    tg::command::{Entities, TextArgs},
    tg::permissions::*,
    util::error::Result,
    util::string::Speak,
};
use botapi::gen_types::{Message, UpdateExt};

use futures::FutureExt;
use humantime::format_duration;
use macros::lang_fmt;
use sea_orm_migration::MigrationTrait;

metadata!("Warns",
    r#"
    Keep your users in line with warnings! Good for pressuring people not to say the word "bro"

    Use the /warn command by either passing a mention/username or by replying to a user to warn.  
    After a user gets a set amount of warnings \(default 3\) the action specified by the /warnmode will
    be applied. The default action is to mute the user.
 
    "#,
    { command = "warn", help = "Warns a user"},
    { command = "warns", help = "Get warn count of a user"},
    { command = "clearwarns", help = "Delete all warns for a user"},
    { command = "warntime", help = "Sets time before warns expire. Usage: /warntime 6m for 6 minutes.
        Use /warntime clear to never expire"},
    { command = "warnmode", help = "Set the action when max warns are reached. Can be 'mute', 'ban' or 'shame'"},
    { command = "warnlimit", help = "Sets the number of warns before an action is taken." }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}
pub async fn warn<'a>(
    message: &Message,
    entities: &Entities<'a>,
    args: &TextArgs<'a>,
    lang: Lang,
) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;

    action_message(message, entities, Some(args), |message, user, args| {
        async move {
            if user.is_admin(message.get_chat_ref()).await? {
                return Err(BotError::speak(
                    &lang_fmt!(lang, "warnadmin"),
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

            warn_with_action(message, user, reason, None).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn warns<'a>(message: &Message, entities: &Entities<'a>, lang: Lang) -> Result<()> {
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;

    action_message(message, entities, None, |message, user, _| {
        async move {
            let warns = get_warns(message, user).await?;
            let list = warns
                .into_iter()
                .map(|w| {
                    format!(
                        "Reason: {}",
                        w.reason.unwrap_or_else(|| lang_fmt!(lang, "noreason"))
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");
            message
                .reply(lang_fmt!(lang, "warns", user.name_humanreadable(), list))
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
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    if let Ok(Some(time)) = parse_duration(&Some(args.as_slice()), message.get_chat().get_id()) {
        set_warn_time(message.get_chat_ref(), Some(time.num_seconds())).await?;
        let time = format_duration(time.to_std()?);
        message.reply(format!("Set warn time to {}", time)).await?;
    } else if args.text.trim() == "clear" {
        set_warn_time(message.get_chat_ref(), None).await?;
        message.reply("Cleared warn time").await?;
    } else {
        message.reply("Specify a time").await?;
    }
    Ok(())
}

async fn cmd_warn_mode<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    set_warn_mode(message.get_chat_ref(), args.text).await?;
    message
        .reply(format!("Set warn mode {}", args.text))
        .await?;
    Ok(())
}

async fn cmd_warn_limit<'a>(message: &Message, args: &TextArgs<'a>) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    match i32::from_str_radix(args.text.trim(), 10) {
        Ok(num) => {
            if num > 0 {
                set_warn_limit(message.get_chat_ref(), num).await?;
                message.reply(format!("set warn limit to {}", num)).await?;
            } else {
                message
                    .reply("Negative warn limits don't make sense")
                    .await?;
            }
        }
        Err(_) => {
            message.speak("Enter a number").await?;
        }
    }
    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, entities, args, message, lang)) = ctx.cmd() {
        match cmd {
            "warn" => warn(message, &entities, args, lang.clone()).await,
            "warns" => warns(message, &entities, lang.clone()).await,
            "clearwarns" => clear(message, &entities).await,
            "warntime" => set_time(message, args).await,
            "warnmode" => cmd_warn_mode(message, args).await,
            "warnlimit" => cmd_warn_limit(message, args).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    if let Some(cmd) = cmd {
        handle_command(cmd).await?;
    }
    Ok(())
}
