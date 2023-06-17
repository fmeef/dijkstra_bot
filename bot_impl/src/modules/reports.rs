use crate::statics::TG;
use crate::tg::command::Context;

use crate::tg::permissions::*;
use crate::tg::user::GetUser;
use crate::util::error::BotError;
use crate::util::string::should_ignore_chat;
use crate::{metadata::metadata, tg::admin_helpers::*, util::error::Result};
use botapi::gen_types::{MessageEntity, MessageEntityBuilder};
use futures::FutureExt;
use macros::textentity_fmt;
use sea_orm_migration::MigrationTrait;

metadata!("Reports",
    r#"
    Allow users to report wrongdoers to admins. Each report notifies up to 4 admins.  
    "#,
    { command = "report", help = "Reports a user"}

);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn report(ctx: &Context) -> Result<()> {
    if let Some(chat) = ctx.chat() {
        if should_ignore_chat(chat.get_id()).await? {
            return Ok(());
        }

        is_group_or_die(ctx.message()?.get_chat_ref()).await?;
        if ctx
            .message()?
            .get_from()
            .is_admin(ctx.message()?.get_chat_ref())
            .await?
        {
            return Err(BotError::Generic("Admins can't warn".into()));
        }

        ctx.action_message(|ctx, user, _| {
            async move {
                if let Some(chat) = ctx.chat() {
                    if user.is_admin(chat).await? {
                        return Err(BotError::speak(
                            "I am not going to report an admin, what the FLOOP",
                            chat.get_id(),
                        ));
                    }
                    let mut admins = ctx
                        .message()?
                        .get_chat()
                        .get_cached_admins()
                        .await?
                        .values()
                        .map(|a| {
                            MessageEntityBuilder::new(0, 0)
                                .set_type("text_mention".to_owned())
                                .set_user(a.get_user().into_owned())
                                .build()
                        })
                        .collect::<Vec<MessageEntity>>();

                    let mention = user.mention().await?;
                    let te = textentity_fmt!(ctx.try_get()?.lang, "reported", mention);
                    let (text, entities) = te.textentities();
                    admins.extend_from_slice(entities.as_slice());
                    TG.client()
                        .build_send_message(chat.get_id(), text)
                        .reply_to_message_id(ctx.message()?.get_message_id())
                        .entities(&admins)
                        .build()
                        .await?;
                }
                Ok(())
            }
            .boxed()
        })
        .await?;
    }
    Ok(())
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some((cmd, _, _, _, _)) = ctx.cmd() {
        match cmd {
            "report" => report(ctx).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;

    Ok(())
}
