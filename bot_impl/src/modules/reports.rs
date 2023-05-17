use crate::statics::TG;
use crate::tg::command::Context;
use crate::tg::markdown::MarkupType;
use crate::tg::permissions::*;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::util::string::{should_ignore_chat, Lang};
use crate::{metadata::metadata, tg::admin_helpers::*, tg::command::Entities, util::error::Result};
use botapi::gen_types::{Message, MessageEntity, MessageEntityBuilder, UpdateExt};
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

pub async fn report<'a>(message: &Message, entities: &Entities<'a>, lang: Lang) -> Result<()> {
    if should_ignore_chat(message.get_chat().get_id()).await? {
        return Ok(());
    }
    is_group_or_die(message.get_chat_ref()).await?;
    if message.get_from().is_admin(message.get_chat_ref()).await? {
        return Err(BotError::Generic("Admins can't warn".into()));
    }

    action_message(message, entities, None, |message, user, _| {
        async move {
            if user.is_admin(message.get_chat_ref()).await? {
                return Err(BotError::speak(
                    "I am not going to report an admin, what the FLOOP",
                    message.get_chat().get_id(),
                ));
            }
            let mut admins = message
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

            let name = user.name_humanreadable();
            let mention = MarkupType::TextMention(user.to_owned()).text(&name);
            let te = textentity_fmt!(lang, "reported", mention);
            let (text, entities) = te.textentities();
            admins.extend_from_slice(entities.as_slice());
            TG.client()
                .build_send_message(message.get_chat().get_id(), text)
                .reply_to_message_id(message.get_message_id())
                .entities(&admins)
                .build()
                .await?;

            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

async fn handle_command<'a>(ctx: &Context<'a>) -> Result<()> {
    if let Some((cmd, entities, _, message, lang)) = ctx.cmd() {
        match cmd {
            "report" => report(message, &entities, lang.clone()).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    if let Some(cmd) = cmd {
        handle_command(cmd).await?;
    }
    Ok(())
}
