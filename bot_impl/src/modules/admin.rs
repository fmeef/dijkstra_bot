use crate::tg::command::Cmd;
use crate::tg::markdown::EntityMessage;
use crate::tg::permissions::*;
use crate::tg::user::GetUser;
use crate::{
    metadata::metadata,
    tg::command::Context,
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};

use futures::{stream, StreamExt, TryStreamExt};

use macros::{entity_fmt, lang_fmt};
use sea_orm_migration::MigrationTrait;
metadata!("Admin",
    r#"
    Manage admins using the bot. Promote or demote users without having to google how to do it on iOS.

    The promote and demote command either take a username/mention as a parameter or allow replying to
    a message from the user that you want to interact with. Users promoted in this way can only have the
    same permissions as the bot. The bot cannot demote users that have been promoted by another bot or
    admin.

    The /admincache command is used to refresh the cached admin list if the admins of a group were
    changed recently. This is to avoid spamming the telegram api. Use this command if the bot
    does not correctly recognize an admin
    "#,
    { command = "admincache", help = "Refresh the cached list of admins" },
    { command = "admins", help = "Get a list of admins" },
    { command = "promote", help = "Promote a user to admin"},
    { command = "demote", help = "Demote a user" }
);
pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn promote(context: &Context) -> Result<()> {
    context.check_permissions(|v| v.can_promote_members).await?;
    context
        .action_message(move |ctx, user, _| async move {
            let message = ctx.message()?;
            if let Some(chat) = ctx.chat() {
                chat.promote(user).await?;
                let mention = user.mention().await?;
                message
                    .speak_fmt(entity_fmt!(ctx, "promote", mention))
                    .await?;
            }
            Ok(())
        })
        .await?;

    Ok(())
}

async fn demote<'a>(context: &'a Context) -> Result<()> {
    context.check_permissions(|p| p.can_promote_members).await?;
    context
        .action_message(|ctx, user, _| async move {
            if let Some(chat) = ctx.chat() {
                match chat.demote(user).await {
                    Err(err) => {
                        ctx.reply(format!("failed to demote user: {}", err.get_tg_error()))
                            .await?;
                    }
                    Ok(_) => {
                        let mention = user.mention().await?;
                        ctx.speak_fmt(entity_fmt!(ctx, "demote", mention)).await?;
                    }
                }
            }

            Ok(())
        })
        .await?;

    Ok(())
}

async fn listadmins(ctx: &Context) -> Result<()> {
    ctx.is_group_or_die().await?;
    let message = ctx.message()?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let admins = message.get_chat().get_cached_admins().await?;
    let header = lang_fmt!(lang, "foundadmins", admins.len());
    let mut builder = EntityMessage::new(message.get_chat().get_id());
    builder.builder.text(header);
    let body = stream::iter(admins.values().filter(|p| !p.is_anon_admin()))
        .then(|v| async move { v.get_user().mention().await })
        .try_fold(builder, |mut entities, value| async move {
            entities.builder.text("\n");
            entities.builder.regular(value);
            Ok(entities)
        })
        .await?;

    message.speak_fmt(body).await?;
    Ok(())
}

async fn admincache(ctx: &Context) -> Result<()> {
    ctx.is_group_or_die().await?;
    let message = ctx.message()?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    ctx.force_refresh_cached_admins().await?;
    message.speak(lang_fmt!(lang, "refreshac")).await?;

    Ok(())
}
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;
    Ok(())
}
async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "admincache" => admincache(ctx).await,
            "admins" => listadmins(ctx).await,
            "promote" => promote(ctx).await,
            "demote" => demote(ctx).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}
