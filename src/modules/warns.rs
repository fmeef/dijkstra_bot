use crate::tg::command::{Cmd, Context};
use crate::tg::markdown::remove_fillings;
use crate::tg::user::{GetUser, Username};
use crate::util::error::Fail;

use crate::{
    metadata::metadata, tg::admin_helpers::*, tg::command::TextArgs, tg::permissions::*,
    util::error::Result, util::string::Speak,
};

use humantime::format_duration;
use macros::{entity_fmt, lang_fmt, update_handler};

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

pub async fn warn(context: &Context) -> Result<()> {
    context
        .check_permissions(|p| p.can_restrict_members)
        .await?;

    context
        .action_user(|ctx, user, args| async move {
            if user.is_admin(ctx.message()?.get_chat_ref()).await? {
                return ctx.fail(lang_fmt!(ctx.try_get()?.lang, "warnadmin"));
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

            ctx.warn_with_action(user, reason, None).await?;
            Ok(())
        })
        .await?;
    Ok(())
}

pub async fn warns(context: &Context) -> Result<()> {
    if let Some(v) = context.get() {
        context.is_group_or_die().await?;
        let chat = v.chat;
        let lang = v.lang;
        self_admin_or_die(&chat).await?;
        context
            .action_user(|ctx, user, _| async move {
                let warns = get_warns(ctx.try_get()?.chat, user).await?;
                let list = warns
                    .into_iter()
                    .map(|w| {
                        lang_fmt!(
                            lang,
                            "warnsline",
                            w.reason.unwrap_or_else(|| lang_fmt!(lang, "noreason"))
                        )
                    })
                    .map(|v| remove_fillings(&v))
                    .collect::<Vec<String>>()
                    .join("\n");

                let mention = user.mention().await?;
                ctx.reply_fmt(entity_fmt!(context, "warns", mention, list))
                    .await?;
                Ok(())
            })
            .await?;
    }
    Ok(())
}

pub async fn clear<'a>(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    ctx.is_group_or_die().await?;
    self_admin_or_die(&message.get_chat()).await?;
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    ctx.action_user(|ctx, user, _| async move {
        clear_warns(ctx.message()?.get_chat_ref(), user).await?;

        ctx.reply_fmt(entity_fmt!(ctx, "clearwarns", user.mention().await?))
            .await?;
        Ok(())
    })
    .await?;

    Ok(())
}

async fn set_time<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let message = ctx.message()?;
    let chat = ctx.try_get()?.chat.name_humanreadable();
    if let Ok(Some(time)) = ctx.parse_duration(&Some(args.as_slice())) {
        set_warn_time(message.get_chat_ref(), Some(time.num_seconds())).await?;
        let time = format_duration(time.to_std()?);
        message.reply(format!("Set warn time to {}", time)).await?;
    } else if args.text.trim() == "clear" {
        set_warn_time(message.get_chat_ref(), None).await?;
        message
            .reply(lang_fmt!(ctx.lang(), "cleartime", chat))
            .await?;
    } else {
        message.reply(lang_fmt!(ctx.lang(), "specifytime")).await?;
    }
    Ok(())
}

async fn cmd_warn_mode<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let message = ctx.message()?;
    let chat = ctx.try_get()?.chat.name_humanreadable();
    set_warn_mode(message.get_chat_ref(), args.text).await?;
    message
        .reply(lang_fmt!(ctx.lang(), "warnmode", args.text, chat))
        .await?;
    Ok(())
}

async fn cmd_warn_limit<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let message = ctx.message()?;
    let chat = ctx.try_get()?.chat.name_humanreadable();
    match i32::from_str_radix(args.text.trim(), 10) {
        Ok(num) => {
            if num > 0 {
                set_warn_limit(message.get_chat_ref(), num).await?;
                message
                    .reply(lang_fmt!(ctx.lang(), "warnlimit", num, chat))
                    .await?;
            } else {
                message.reply(lang_fmt!(ctx.lang(), "negwarns")).await?;
            }
        }
        Err(_) => {
            message.speak(lang_fmt!(ctx.lang(), "nan")).await?;
        }
    }
    Ok(())
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, ref args, .. }) = ctx.cmd() {
        match cmd {
            "warn" => warn(ctx).await,
            "warns" => warns(ctx).await,
            "clearwarns" => clear(ctx).await,
            "warntime" => set_time(ctx, args).await,
            "warnmode" => cmd_warn_mode(ctx, args).await,
            "warnlimit" => cmd_warn_limit(ctx, args).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

#[update_handler]
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}
