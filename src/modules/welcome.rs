use crate::persist::core::media::get_media_type;
use crate::persist::core::{entity, welcomes};
use crate::statics::{DB, REDIS};
use crate::tg::command::{Cmd, Context, TextArgs};
use crate::tg::markdown::MarkupBuilder;
use crate::tg::permissions::*;
use crate::util::error::{BotError, Result};
use crate::util::string::Lang;
use crate::{metadata::metadata, util::string::Speak};
use botapi::gen_types::Message;
use macros::{lang_fmt, update_handler};
use redis::AsyncCommands;
use sea_orm::entity::ActiveValue::{NotSet, Set};
use sea_orm::EntityTrait;

use sea_query::OnConflict;

metadata!("Welcome",
    r#"
    Welcomes users with custom message. Welcome messages are send when a user joins and
    goodbye messages are sent when a user leaves. Note: this only works in groups with 50
    or fewer members. Groups with more than 50 members will not send welcome messages.  

    [*Example:]  
    /welcome on  
    /setwelcome Hi there \{mention\}, welcome to \{chatname\}
    
    "#,
    { command = "welcome", help = "Usage: welcome \\<on/off\\>. Enables or disables welcome" },
    { command = "setwelcome", help = "Sets the welcome text. Reply to a message or media to set"},
    { command = "setgoodbye", help = "Sets the goodbye message for when a user leaves"},
    { command = "resetwelcome", help = "Resets welcome and goodbye messages to default" }
);

async fn get_model<'a>(
    message: &'a Message,
    args: &'a TextArgs<'a>,
    goodbye: bool,
) -> Result<welcomes::ActiveModel> {
    let (message, text, extra) = if let Some(message) = message.get_reply_to_message() {
        (
            message,
            message.get_text(),
            message.get_entities().map(|v| v.to_owned()),
        )
    } else {
        (message, Some(args.text), None)
    };

    let (text, entity_id) = if let Some(text) = text {
        let (text, entities, buttons) = MarkupBuilder::new(extra)
            .set_text(text.to_owned())
            .filling(false)
            .header(false)
            .build_murkdown_nofail()
            .await;
        log::info!("welcome get with buttons {:?}", buttons.get());
        let entity_id = entity::insert(*DB, &entities, buttons).await?;
        (Some(text), entity_id)
    } else {
        (None, None)
    };
    let (media_id, media_type) = get_media_type(message)?;
    let res = if goodbye {
        welcomes::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            text: NotSet,
            media_id: NotSet,
            media_type: NotSet,
            goodbye_text: Set(text.map(|t| t.to_owned())),
            goodbye_media_id: Set(media_id),
            goodbye_media_type: Set(Some(media_type)),
            enabled: NotSet,
            welcome_entity_id: NotSet,
            goodbye_entity_id: Set(entity_id),
        }
    } else {
        welcomes::ActiveModel {
            chat: Set(message.get_chat().get_id()),
            text: Set(text.map(|t| t.to_owned())),
            media_id: Set(media_id),
            media_type: Set(Some(media_type)),
            goodbye_text: NotSet,
            goodbye_media_id: NotSet,
            goodbye_media_type: NotSet,
            enabled: NotSet,
            welcome_entity_id: Set(entity_id),
            goodbye_entity_id: NotSet,
        }
    };

    Ok(res)
}

async fn enable_welcome<'a>(message: &Message, args: &TextArgs<'a>, lang: &Lang) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    let enabled = match args.args.first().map(|v| v.get_text()) {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some("yes") => Ok(true),
        Some("no") => Ok(false),
        _ => Err(BotError::speak(
            lang_fmt!(lang, "welcomeinvalid"),
            message.get_chat().get_id(),
            Some(message.message_id),
        )),
    }?;
    let model = welcomes::ActiveModel {
        chat: Set(message.get_chat().get_id()),
        text: NotSet,
        media_id: NotSet,
        media_type: NotSet,
        goodbye_text: NotSet,
        goodbye_media_id: NotSet,
        goodbye_media_type: NotSet,
        enabled: Set(enabled),
        welcome_entity_id: NotSet,
        goodbye_entity_id: NotSet,
    };

    welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::column(welcomes::Column::Chat)
                .update_column(welcomes::Column::Enabled)
                .to_owned(),
        )
        .exec_with_returning(*DB)
        .await?;
    REDIS.sq(|q| q.del(&key)).await?;
    message.reply("Enabled welcome").await?;
    Ok(())
}

async fn set_goodbye<'a>(message: &Message, args: &TextArgs<'a>, lang: &Lang) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let model = get_model(message, args, true).await?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    log::info!("save goodbye: {}", key);
    let model = welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([welcomes::Column::Chat])
                .update_columns([
                    welcomes::Column::GoodbyeText,
                    welcomes::Column::GoodbyeMediaId,
                    welcomes::Column::GoodbyeMediaType,
                    welcomes::Column::WelcomeEntityId,
                    welcomes::Column::GoodbyeEntityId,
                ])
                .to_owned(),
        )
        .exec_with_returning(*DB)
        .await?;
    let text = if let Some(text) = model.text.as_ref() {
        lang_fmt!(lang, "setgoodbye", text)
    } else {
        lang_fmt!(lang, "setgoodbye", "*media*")
    };
    REDIS.sq(|q| q.del(&key)).await?;

    message.speak(text).await?;
    Ok(())
}

async fn set_welcome<'a>(message: &Message, args: &TextArgs<'a>, lang: &Lang) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;

    let model = get_model(message, args, false).await?;
    let key = format!("welcome:{}", message.get_chat().get_id());
    log::info!("save welcome: {}", key);
    let model = welcomes::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([welcomes::Column::Chat])
                .update_columns([
                    welcomes::Column::Text,
                    welcomes::Column::MediaId,
                    welcomes::Column::MediaType,
                    welcomes::Column::WelcomeEntityId,
                    welcomes::Column::GoodbyeEntityId,
                ])
                .to_owned(),
        )
        .exec_with_returning(*DB)
        .await?;

    let text = if let Some(text) = model.text.as_ref() {
        lang_fmt!(lang, "setwelcome", text)
    } else {
        lang_fmt!(lang, "setwelcome", "*media*")
    };
    REDIS.sq(|q| q.del(&key)).await?;
    message.speak(text).await?;
    Ok(())
}

async fn handle_command(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        lang,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "setwelcome" => set_welcome(message, args, lang).await?,
            "setgoodbye" => set_goodbye(message, args, lang).await?,
            "welcome" => enable_welcome(message, args, lang).await?,
            "resetwelcome" => reset_welcome(message, lang).await?,
            _ => (),
        };
    }
    Ok(())
}

async fn reset_welcome(message: &Message, lang: &Lang) -> Result<()> {
    message.check_permissions(|p| p.can_change_info).await?;
    let chat = message.get_chat().get_id();
    let key = format!("welcome:{}", chat);

    welcomes::Entity::delete_by_id(chat).exec(*DB).await?;
    REDIS.sq(|q| q.del(&key)).await?;
    message.speak(lang_fmt!(lang, "resetwelcome")).await?;
    Ok(())
}

#[update_handler]
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;
    Ok(())
}
