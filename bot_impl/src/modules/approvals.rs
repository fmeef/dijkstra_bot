use crate::statics::TG;
use crate::tg::admin_helpers::{action_message, approve, get_approvals, unapprove};
use crate::tg::command::{Context, Entities, TextArgs};
use crate::tg::permissions::*;

use crate::tg::markdown::{MarkupBuilder, MarkupType};
use crate::tg::user::{get_user, GetUser, Username};
use crate::util::error::Result;

use crate::metadata::metadata;
use crate::util::string::{Lang, Speak};
use botapi::gen_types::{Message, UpdateExt, UserBuilder};

use futures::FutureExt;

use macros::entity_fmt;
use sea_orm_migration::MigrationTrait;
metadata!("Approvals",
    r#"
    Approvals are a tool to allow specific users to be ignored by automated admin actions  
    "#,
    { command = "approve", help = "Approves a user"},
    { command = "unapprove", help = "Removals approval" },
    { command = "listapprovals", help = "List all approvals for current chat"}
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
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    action_message(message, entities, Some(args), |message, user, _| {
        async move {
            if let Some(user) = user.get_cached_user().await? {
                approve(message.get_chat_ref(), &user).await?;
                let name = user.name_humanreadable();
                message
                    .speak_fmt(entity_fmt!(
                        lang,
                        message.get_chat().get_id(),
                        "approved",
                        MarkupType::TextMention(user.clone()).text(&name)
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

async fn cmd_unapprove<'a>(
    message: &Message,
    args: &TextArgs<'a>,
    entities: &Entities<'a>,
    lang: Lang,
) -> Result<()> {
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    action_message(message, entities, Some(args), |message, user, _| {
        async move {
            unapprove(message.get_chat_ref(), user).await?;
            let name = user.mention().await?;
            message
                .speak_fmt(entity_fmt!(
                    lang,
                    message.get_chat().get_id(),
                    "unapproved",
                    name
                ))
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn command_list<'a>(context: &Context<'a>) -> Result<()> {
    context
        .message
        .check_permissions(|p| p.can_manage_chat)
        .await?;
    let mut res = MarkupBuilder::new();
    let chat_name = context.chat.name_humanreadable();
    res.bold(format!("Approved users for {}\n", chat_name));
    for (userid, name) in get_approvals(context.chat).await? {
        if let Some(user) = get_user(userid).await? {
            let name = user.name_humanreadable();
            res.text_mention(&name, user, None);
        } else {
            let n = name.clone();
            let user = UserBuilder::new(userid, false, name).build();
            res.text_mention(&n, user, None);
        };
        res.text("\n");
    }
    let (msg, entities) = res.build();
    let msg = TG
        .client
        .build_send_message(context.chat.get_id(), msg)
        .entities(entities);

    context.message.reply_fmt(msg).await?;

    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, entities, args, message, lang)) = ctx.cmd() {
        match cmd {
            "approve" => cmd_approve(message, args, entities, lang.clone()).await?,
            "unapprove" => cmd_unapprove(message, args, entities, lang.clone()).await?,
            "listapprovals" => command_list(ctx).await?,
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
