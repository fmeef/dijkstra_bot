use crate::persist::admin::actions;

use crate::statics::TG;
use crate::tg::user::Username;
use crate::util::error::BotError;
use crate::util::string::should_ignore_chat;
use crate::{
    metadata::metadata,
    tg::admin_helpers::*,
    tg::{
        command::{Command, Entities, TextArgs},
        dialog::dialog_or_default,
    },
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{Message, MessageEntity, MessageEntityBuilder, UpdateExt, User};

use futures::FutureExt;
use macros::rlformat;
use sea_orm_migration::MigrationTrait;

metadata!("Reports",
    { command = "report", help = "Reports a user"}

);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn report<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    if should_ignore_chat(message.get_chat().get_id()).await? {
        return Ok(());
    }
    if message.get_from().is_admin(message.get_chat_ref()).await? {
        return Err(BotError::Generic("Admins can't warn".into()));
    }

    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    action_message(message, entities, None, |message, user, _| {
        async move {
            if user.is_admin(message.get_chat_ref()).await? {
                return Err(BotError::speak(
                    "I am not going to report an admin, what the FLOOP",
                    message.get_chat().get_id(),
                ));
            }
            let name = user.name_humanreadable();
            let admins = message
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
            TG.client()
                .build_send_message(
                    message.get_chat().get_id(),
                    &rlformat!(lang, "reported", name),
                )
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

async fn handle_command<'a>(message: &Message, cmd: Option<&Command<'a>>) -> Result<()> {
    if let Some(&Command {
        cmd, ref entities, ..
    }) = cmd
    {
        log::info!("admin command {}", cmd);

        match cmd {
            "report" => report(message, &entities).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update<'a>(update: &UpdateExt, cmd: Option<&Command<'a>>) -> Result<()> {
    match update {
        UpdateExt::Message(ref message) => handle_command(message, cmd).await?,
        _ => (),
    };
    Ok(())
}