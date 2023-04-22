use crate::{
    metadata::metadata,
    tg::{admin_helpers::*, command::Context, markdown::MarkupType, user::Username},
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
    Manage admins using the bot. Promote or demote users without having to google how to do it on iOS
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
    if let (Some(command), Some(message)) = (context.command.as_ref(), context.message) {
        message.group_admin_or_die().await?;
        let lang = context.lang.clone();
        action_message(message, &command.entities, None, |message, user, _| {
            async move {
                message.get_chat().promote(user.get_id()).await?;

                let name = user.name_humanreadable();
                let mention = MarkupType::TextMention(user.to_owned()).text(&name);
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
    if let (Some(command), Some(message)) = (context.command.as_ref(), context.message) {
        message.group_admin_or_die().await?;
        let lang = context.lang.clone();

        action_message(message, &command.entities, None, |message, user, _| {
            async move {
                message.get_chat().demote(user.get_id()).await?;

                let name = user.name_humanreadable();
                let mention = MarkupType::TextMention(user.to_owned()).text(&name);
                message
                    .speak_fmt(entity_fmt!(
                        lang,
                        message.get_chat().get_id(),
                        "demote",
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
