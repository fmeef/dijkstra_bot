use crate::tg::admin_helpers::{approve, get_approvals, unapprove};
use crate::tg::command::{Cmd, Context};
use crate::tg::permissions::*;

use crate::tg::markdown::EntityMessage;
use crate::tg::user::{get_user, GetUser, Username};
use crate::util::error::{BotError, Result, SpeakErr};

use crate::metadata::metadata;
use crate::util::string::Speak;
use botapi::gen_types::UserBuilder;

use macros::{entity_fmt, lang_fmt, update_handler};
metadata!("Approvals",
    r#"
    Approvals are a tool to allow specific users to be ignored by automated admin actions
    "#,
    { command = "approve", help = "Approves a user"},
    { command = "unapprove", help = "Removals approval" },
    { command = "listapprovals", help = "List all approvals for current chat"}
);

async fn cmd_approve<'a>(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    ctx.action_user(|ctx, user, _| async move {
        if let Some(user) = user.get_cached_user().await? {
            approve(ctx.message()?.get_chat(), &user).await?;
            let name = user.mention().await?;
            ctx.reply_fmt(entity_fmt!(ctx, "approved", name)).await?;
        }
        Ok(())
    })
    .await
    .speak_err_raw(ctx, |v| match v {
        BotError::UserNotFound => Some(lang_fmt!(ctx, "failuser", "approve")),
        _ => None,
    })
    .await?;
    Ok(())
}

async fn cmd_unapprove(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    ctx.action_user(|ctx, user, _| async move {
        unapprove(ctx.message()?.get_chat(), user).await?;
        let name = user.mention().await?;
        ctx.reply_fmt(entity_fmt!(ctx, "unapproved", name)).await?;

        Ok(())
    })
    .await
    .speak_err_raw(ctx, |v| match v {
        BotError::UserNotFound => Some(lang_fmt!(ctx, "failuser", "unapprove")),
        _ => None,
    })
    .await?;
    Ok(())
}

async fn command_list<'a>(context: &Context) -> Result<()> {
    context.check_permissions(|p| p.can_manage_chat).await?;

    if let Some(chat) = context.chat() {
        let mut res = EntityMessage::new(chat.get_id());
        let chat_name = chat.name_humanreadable();
        res.builder
            .bold(format!("Approved users for {}\n", chat_name));
        for (userid, name) in get_approvals(chat).await? {
            if let Some(user) = get_user(userid).await? {
                let name = user.name_humanreadable().into_owned();
                res.builder.text_mention(&name, user, None);
            } else {
                let n = name.clone();
                let user = UserBuilder::new(userid, false, name).build();
                res.builder.text_mention(&n, user, None);
            };
            res.builder.text("\n");
        }

        context.reply_fmt(res).await?;
    }

    Ok(())
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "approve" => cmd_approve(ctx).await?,
            "unapprove" => cmd_unapprove(ctx).await?,
            "listapprovals" => command_list(ctx).await?,
            _ => (),
        };
    }
    Ok(())
}

#[update_handler]
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}
