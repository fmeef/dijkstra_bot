use macros::{lang_fmt, update_handler};

use crate::persist::admin::gbans;
use crate::tg::command::{Cmd, Context};
use crate::tg::federations::gban_user;
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::GetUser;
use crate::util::error::{BotError, Result, SpeakErr};
use crate::{metadata::metadata, util::string::Speak};

metadata!("Global Bans",
    r#"
    Global bans \(gbans\) ban a user across every chat the bot is in. This is a drastic action
    and therefore can only be taken by support users or the owner of the bot.
    "#,
    { command = "gban", help = "Ban a user in all chats" },
    { command = "ungban", help = "Unban a user in all chats" }
);

async fn ungban(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.is_support).await?;
    ctx.action_user(|ctx, user, _| async move {
        if let Some(user) = user.get_cached_user().await? {
            ctx.ungban_user(user.get_id()).await?;
            ctx.reply("user ungbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await
    .speak_err_raw(ctx, |v| match v {
        BotError::UserNotFound => Some(lang_fmt!(ctx, "failuser", "ungban")),
        _ => None,
    })
    .await?;
    Ok(())
}
async fn gban(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.is_support).await?;
    ctx.action_user(|ctx, user, args| async move {
        if let Some(user) = user.get_cached_user().await? {
            let mut model = gbans::Model::new(user.get_id());

            model.reason = args
                .map(|v| v.text.trim().to_owned())
                .and_then(|v| (!v.is_empty()).then_some(v));
            gban_user(model, user).await?;
            ctx.reply("user gbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await
    .speak_err_raw(ctx, |v| match v {
        BotError::UserNotFound => Some(lang_fmt!(ctx, "failuser", "gban")),
        _ => None,
    })
    .await?;
    Ok(())
}

#[update_handler]
pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "gban" => gban(ctx).await,
            "ungban" => ungban(ctx).await,
            _ => Ok(()),
        }?;
    }

    Ok(())
}
