use crate::{
    metadata::metadata,
    statics::TG,
    tg::admin_helpers::*,
    tg::{
        command::{Context, Entities, TextArgs},
        markdown::MarkupType,
        user::Username,
    },
    util::error::Result,
    util::string::{get_chat_lang, Speak},
};
use botapi::gen_types::{ChatPermissionsBuilder, Message, UpdateExt};
use futures::FutureExt;
use macros::{entity_fmt, lang_fmt};
use sea_orm_migration::MigrationTrait;

metadata!("Bans",
    r#"
    Ever had a problem with users being too annoying? Has someone admitted to using
    a turing-complete language \(known to be a dangerous piracy tool\) or downloaded a
    yellow terrorist app? This module is the solution!  
    Mute or ban users, punish blue-texters with /kickme, etc
    "#,
    { command = "kickme", help = "Send a free course on termux hacking"},
    { command = "mute", help = "Mute a user"},
    { command = "unmute", help = "Unmute a user"},
    { command = "ban", help = "Bans a user"},
    { command = "unban", help = "Unbans a user"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn unban_cmd<'a>(message: &'a Message, entities: &Entities<'a>) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    action_message(message, entities, None, |message, user, _| {
        async move {
            unban(message, user).await?;
            let name = user.name_humanreadable();
            let entity = MarkupType::TextMention(user.to_owned()).text(&name);
            message
                .speak_fmt(entity_fmt!(
                    lang,
                    message.get_chat().get_id(),
                    "unbanned",
                    entity
                ))
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn ban_cmd<'a>(
    message: &'a Message,
    entities: &Entities<'a>,
    args: &'a TextArgs<'a>,
) -> Result<()> {
    message.group_admin_or_die().await?;
    action_message(message, entities, Some(args), |message, user, args| {
        async move {
            let duration = parse_duration(&args, message.get_chat().get_id())?;
            ban(message, user, duration).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn mute_cmd<'a>(
    message: &Message,
    entities: &Entities<'a>,
    args: &TextArgs<'a>,
) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(false)
        .set_can_send_audios(false)
        .set_can_send_documents(false)
        .set_can_send_photos(false)
        .set_can_send_videos(false)
        .set_can_send_video_notes(false)
        .set_can_send_polls(false)
        .set_can_send_voice_notes(false)
        .set_can_send_other_messages(false)
        .build();
    let user = change_permissions_message(message, &entities, permissions, args).await?;
    let name = user.name_humanreadable();
    let mention = MarkupType::TextMention(user).text(&name);

    message
        .speak_fmt(entity_fmt!(
            lang,
            message.get_chat().get_id(),
            "muteuser",
            mention
        ))
        .await?;
    //  message.speak(lang_fmt!(lang, "muteuser")).await?;

    Ok(())
}

pub async fn unmute_cmd<'a>(
    message: &'a Message,
    entities: &Entities<'a>,
    args: &'a TextArgs<'a>,
) -> Result<()> {
    message.group_admin_or_die().await?;
    let lang = get_chat_lang(message.get_chat().get_id()).await?;

    let permissions = ChatPermissionsBuilder::new()
        .set_can_send_messages(true)
        .set_can_send_audios(true)
        .set_can_send_documents(true)
        .set_can_send_photos(true)
        .set_can_send_videos(true)
        .set_can_send_video_notes(true)
        .set_can_send_polls(true)
        .set_can_send_voice_notes(true)
        .set_can_send_other_messages(true)
        .build();

    let user = change_permissions_message(message, &entities, permissions, args).await?;

    let name = user.name_humanreadable();
    let mention = MarkupType::TextMention(user).text(&name);
    message
        .speak_fmt(entity_fmt!(
            lang,
            message.get_chat().get_id(),
            "unmuteuser",
            mention
        ))
        .await?;

    Ok(())
}

async fn kickme(message: &Message) -> Result<()> {
    let lang = get_chat_lang(message.get_chat().get_id()).await?;
    is_group_or_die(&message.get_chat()).await?;
    self_admin_or_die(&message.get_chat()).await?;
    if message.get_from().is_admin(&message.get_chat()).await? {
        message.speak(lang_fmt!(lang, "kickadmin")).await?;
    } else {
        if let Some(from) = message.get_from() {
            TG.client()
                .build_ban_chat_member(message.get_chat().get_id(), from.get_id())
                .build()
                .await?;
            TG.client()
                .build_unban_chat_member(message.get_chat().get_id(), from.get_id())
                .build()
                .await?;
            message.speak(lang_fmt!(lang, "kickme")).await?;
        }
    }

    Ok(())
}

async fn handle_command<'a>(ctx: &Option<Context<'a>>) -> Result<()> {
    if let Some(ctx) = ctx {
        if let Some((cmd, entities, args, message)) = ctx.cmd() {
            match cmd {
                "kickme" => kickme(message).await,
                "mute" => mute_cmd(message, &entities, args).await,
                "unmute" => unmute_cmd(message, &entities, args).await,
                "ban" => ban_cmd(message, &entities, args).await,
                "unban" => unban_cmd(message, &entities).await,
                _ => Ok(()),
            }?;
        }
    }
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, cmd: &Option<Context<'a>>) -> Result<()> {
    handle_command(cmd).await
}
