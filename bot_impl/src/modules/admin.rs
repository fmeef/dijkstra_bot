use crate::tg::permissions::*;
use crate::tg::user::GetUser;
use crate::{
    metadata::metadata,
    tg::{admin_helpers::*, command::Context},
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{Message, UpdateExt};
use futures::FutureExt;
use itertools::Itertools;
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

async fn promote<'a>(context: &'a Context<'a>) -> Result<()> {
    if let (Some(command), message) = (context.command.as_ref(), context.message) {
        message.check_permissions(|v| v.can_promote_members).await?;
        let lang = context.lang.clone();
        action_message(message, &command.entities, None, |message, user, _| {
            async move {
                message.get_chat().promote(user).await?;
                let mention = user.mention().await?;
                message
                    .speak_fmt(entity_fmt!(
                        lang,
                        message.get_chat().get_id(),
                        "promote",
                        mention
                    ))
                    .await?;
                Ok(())
            }
            .boxed()
        })
        .await?;
    }
    Ok(())
}

async fn demote<'a>(context: &'a Context<'a>) -> Result<()> {
    if let (Some(command), message) = (context.command.as_ref(), context.message) {
        message.check_permissions(|p| p.can_promote_members).await?;
        let lang = context.lang.clone();

        action_message(message, &command.entities, None, |message, user, _| {
            async move {
                match message.get_chat().demote(user).await {
                    Err(err) => {
                        message
                            .reply(format!("failed to demote user: {}", err.get_tg_error()))
                            .await?;
                    }
                    Ok(_) => {
                        let mention = user.mention().await?;
                        message
                            .speak_fmt(entity_fmt!(
                                lang,
                                message.get_chat().get_id(),
                                "demote",
                                mention
                            ))
                            .await?;
                    }
                }

                Ok(())
            }
            .boxed()
        })
        .await?;
    }
    Ok(())
}

async fn listadmins(message: &Message) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let admins = message.get_chat().get_cached_admins().await?;
    let header = lang_fmt!(lang, "foundadmins", admins.len());
    let body = admins
        .values()
        .map(|v| {
            v.get_user()
                .get_username()
                .map(|u| u.into_owned())
                .unwrap_or_else(|| v.get_user().get_id().to_string())
        })
        .join("\n - ");
    message.speak(format!("{}:\n - {}", header, body)).await?;
    Ok(())
}

async fn admincache(message: &Message) -> Result<()> {
    is_group_or_die(message.get_chat_ref()).await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    message.get_chat().refresh_cached_admins().await?;
    message.speak(lang_fmt!(lang, "refreshac")).await?;
    Ok(())
}
pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    if let Some(cmd) = cmd {
        handle_command(cmd).await?;
    }
    Ok(())
}
async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, _, _, message, _)) = ctx.cmd() {
        match cmd {
            "admincache" => admincache(message).await,
            "admins" => listadmins(message).await,
            "promote" => promote(ctx).await,
            "demote" => demote(ctx).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}
