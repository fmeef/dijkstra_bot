use crate::statics::TG;
use crate::tg::command::{Cmd, Context};

use crate::tg::permissions::*;
use crate::tg::user::GetUser;
use crate::util::error::{BotError, Fail};
use crate::util::string::{should_ignore_chat, Speak};
use crate::{metadata::metadata, util::error::Result};
use botapi::gen_types::{MessageEntity, MessageEntityBuilder};

use macros::{lang_fmt, textentity_fmt, update_handler};

metadata!("Reports",
    r#"
    Allow users to report wrongdoers to admins. Each report notifies up to 4 admins.  
    "#,
    { command = "report", help = "Reports a user"}

);

pub async fn report(ctx: &Context) -> Result<()> {
    if let Some(chat) = ctx.chat() {
        if should_ignore_chat(chat.get_id()).await? {
            return Ok(());
        }

        ctx.is_group_or_die().await?;
        if ctx
            .message()?
            .get_from()
            .is_admin(ctx.message()?.get_chat_ref())
            .await?
        {
            return Err(BotError::Generic("Admins can't warn".into()));
        }

        ctx.action_message_some(|ctx, user, _, _| async move {
            if let Some(chat) = ctx.chat() {
                if let Some(user) = user {
                    if user.is_admin(chat).await? {
                        return ctx.fail("I am not going to report an admin, what the FLOOP");
                    }
                    let mut admins = ctx
                        .message()?
                        .get_chat()
                        .get_cached_admins()
                        .await?
                        .values()
                        .filter(|v| !v.is_anon_admin())
                        .map(|a| {
                            MessageEntityBuilder::new(0, 0)
                                .set_type("text_mention".to_owned())
                                .set_user(a.get_user().into_owned())
                                .build()
                        })
                        .collect::<Vec<MessageEntity>>();

                    let mention = user.mention().await?;
                    let te = textentity_fmt!(ctx, "reported", mention);
                    let (text, entities) = (&te.builder.text, &te.builder.entities);
                    admins.extend_from_slice(entities.as_slice());
                    TG.client()
                        .build_send_message(chat.get_id(), text)
                        .reply_to_message_id(ctx.message()?.get_message_id())
                        .entities(&admins)
                        .build()
                        .await?;
                } else {
                    ctx.reply(lang_fmt!(ctx, "reported_nomention")).await?;
                }
            }
            Ok(())
        })
        .await?;
    }
    Ok(())
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "report" => report(ctx).await,
            _ => Ok(()),
        }?;
    }

    if let Ok(Some(message)) = ctx.message().map(|m| m.get_text_ref()) {
        if let Some(message) = message.trim_start().split_whitespace().next() {
            match message {
                "@admin" => report(ctx).await,
                "@admins" => report(ctx).await,
                "@mods" => report(ctx).await,
                _ => Ok(()),
            }?;
        }
    }
    Ok(())
}

#[update_handler]
pub async fn handle_update(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}
