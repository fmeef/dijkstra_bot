use crate::{
    metadata::metadata,
    statics::TG,
    tg::admin_helpers::*,
    tg::command::Context,
    tg::{permissions::*, user::GetUser},
    util::string::{Lang, Speak},
    util::{
        error::{Result, SpeakErr},
        string::get_chat_lang,
    },
};
use botapi::gen_types::{ChatPermissionsBuilder, Message};
use futures::FutureExt;
use macros::{entity_fmt, lang_fmt};
use sea_orm_migration::MigrationTrait;

metadata!("Bans",
    r#"
    Ever had a problem with users being too annoying? Has someone admitted to using
    a turing-complete language \(known to be a dangerous piracy tool\) or downloaded a
    yellow terrorist app? This module is the solution!  
    Mute or ban users, punish blue-texters with /kickme, etc

    Ban and mute commands take an optional time parameter \(5m, 1d, etc\) and can either take a user
    parameter by mention or @handle or by replying to the user's message.

    [*Examples]  
    [_bans a user for 5 minutes]  
    /ban @username 5m

    [_mutes a user forever]  
    /mute @username
    "#,
    { command = "kickme", help = "Send a free course on termux hacking"},
    { command = "mute", help = "Mute a user"},
    { command = "unmute", help = "Unmute a user"},
    { command = "ban", help = "Bans a user"},
    { command = "unban", help = "Unbans a user"},
    { command = "kick", help = "Kicks a user, they can join again"}
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn unban_cmd(ctx: &Context) -> Result<()> {
    ctx.message()?
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    ctx.action_message(|ctx, user, _| {
        async move {
            if let Some(chat) = ctx.chat() {
                unban(ctx.message()?, user).await?;

                let entity = user.mention().await?;
                ctx.message()?
                    .speak_fmt(entity_fmt!(
                        ctx.try_get()?.lang,
                        chat.get_id(),
                        "unbanned",
                        entity
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

pub async fn ban_cmd(ctx: &Context) -> Result<()> {
    ctx.message()?
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    let lang = get_chat_lang(ctx.message()?.get_chat_ref().get_id()).await?;
    ctx.action_message(|ctx, user, args| {
        async move {
            if let Some(chat) = ctx.chat() {
                let duration = parse_duration(&args, chat.get_id())?;
                ctx.ban(user, duration)
                    .await
                    .speak_err(ctx.message()?.get_chat_ref(), 400, |_| {
                        lang_fmt!(lang, "failban")
                    })
                    .await?;
            }
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn kick_cmd<'a>(ctx: &Context) -> Result<()> {
    ctx.message()?
        .check_permissions(|p| p.can_restrict_members)
        .await?;
    ctx.action_message(|ctx, user, _| {
        async move {
            if let Some(chat) = ctx.chat() {
                kick(user, chat.get_id()).await?;
                let entity = user.mention().await?;
                ctx.message()?
                    .speak_fmt(entity_fmt!(
                        ctx.try_get()?.lang,
                        chat.get_id(),
                        "kicked",
                        entity
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

pub async fn mute_cmd<'a>(ctx: &Context) -> Result<()> {
    ctx.message()?
        .check_permissions(|p| p.can_restrict_members)
        .await?;
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
    let lang = ctx.try_get()?.lang;
    let user = ctx
        .change_permissions_message(permissions)
        .await
        .speak_err(ctx.message()?.get_chat_ref(), 400, |_| {
            lang_fmt!(lang, "failmute")
        })
        .await?;
    let mention = user.mention().await?;

    if let Some(chat) = ctx.chat() {
        ctx.message()?
            .speak_fmt(entity_fmt!(
                ctx.try_get()?.lang,
                chat.get_id(),
                "muteuser",
                mention
            ))
            .await?;
    }

    //  message.speak(lang_fmt!(lang, "muteuser")).await?;

    Ok(())
}

pub async fn unmute_cmd<'a>(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    message
        .check_permissions(|p| p.can_restrict_members)
        .await?;

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

    let lang = ctx.try_get()?.lang;
    let user = ctx
        .change_permissions_message(permissions)
        .await
        .speak_err(message.get_chat_ref(), 400, |_| lang_fmt!(lang, "failmute"))
        .await?;
    let mention = user.mention().await?;
    message
        .speak_fmt(entity_fmt!(
            ctx.try_get()?.lang,
            message.get_chat().get_id(),
            "unmuteuser",
            mention
        ))
        .await?;

    Ok(())
}

async fn kickme(message: &Message, lang: &Lang) -> Result<()> {
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

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some((cmd, _, _, message, lang)) = ctx.cmd() {
        match cmd {
            "kickme" => kickme(message, lang).await,
            "mute" => mute_cmd(ctx).await,
            "unmute" => unmute_cmd(ctx).await,
            "ban" => ban_cmd(ctx).await,
            "unban" => unban_cmd(ctx).await,
            "kick" => kick_cmd(ctx).await,
            _ => Ok(()),
        }?;
    }
    Ok(())
}

pub async fn handle_update<'a>(ctx: &Context) -> Result<()> {
    handle_command(ctx).await
}
