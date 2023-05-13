use crate::tg::admin_helpers::{action_message, approve, unapprove};
use crate::tg::command::{Context, Entities, TextArgs};

use crate::tg::markdown::MarkupType;
use crate::tg::user::Username;
use crate::util::error::Result;

use crate::metadata::metadata;
use crate::util::string::{Lang, Speak};
use botapi::gen_types::{Message, UpdateExt};

use futures::FutureExt;

use macros::entity_fmt;
use sea_orm_migration::MigrationTrait;
metadata!("Approvals",
    r#"
    Approvals are a tool to allow specific users to be ignored by automated admin actions  
    "#,
    { command = "approve", help = "Approves a user"},
    { command = "unapprove", help = "Removals approval" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn cmd_approve<'a>(
    message: &Message,
    args: &TextArgs<'a>,
    entities: &Entities<'a>,
    lang: Lang,
) -> Result<()> {
    action_message(message, entities, Some(args), |message, user, _| {
        async move {
            approve(message.get_chat_ref(), user).await?;
            let name = user.name_humanreadable();
            message
                .speak_fmt(entity_fmt!(
                    lang,
                    message.get_chat().get_id(),
                    "approved",
                    MarkupType::TextMention(user.clone()).text(&name)
                ))
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn cmd_unapprove<'a>(
    message: &Message,
    args: &TextArgs<'a>,
    entities: &Entities<'a>,
    lang: Lang,
) -> Result<()> {
    action_message(message, entities, Some(args), |message, user, _| {
        async move {
            unapprove(message.get_chat_ref(), user).await?;
            let name = user.name_humanreadable();
            message
                .speak_fmt(entity_fmt!(
                    lang,
                    message.get_chat().get_id(),
                    "unapproved",
                    MarkupType::TextMention(user.clone()).text(&name)
                ))
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, entities, args, message, lang)) = ctx.cmd() {
        match cmd {
            "approve" => cmd_approve(message, args, entities, lang.clone()).await?,
            "unapprove" => cmd_unapprove(message, args, entities, lang.clone()).await?,
            _ => (),
        };
    }
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    if let Some(cmd) = cmd {
        handle_command(cmd).await?;
    }
    Ok(())
}
