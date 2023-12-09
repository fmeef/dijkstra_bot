use macros::update_handler;

use crate::persist::admin::gbans;
use crate::tg::admin_helpers::gban_user;
use crate::tg::command::{Cmd, Context};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::GetUser;
use crate::util::error::Result;
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
    ctx.action_message(|ctx, user, _| async move {
        if let Some(user) = user.get_cached_user().await? {
            ctx.ungban_user(user.get_id()).await?;
            ctx.reply("user ungbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await?;
    Ok(())
}
async fn gban(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.is_support).await?;
    ctx.action_message(|ctx, user, args| async move {
        if let Some(user) = user.get_cached_user().await? {
            let mut model = gbans::Model::new(user.get_id());

            model.reason = args
                .map(|v| v.text.trim().to_owned())
                .map(|v| (!v.is_empty()).then(|| v))
                .flatten();
            gban_user(model, user).await?;
            ctx.reply("user gbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
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
