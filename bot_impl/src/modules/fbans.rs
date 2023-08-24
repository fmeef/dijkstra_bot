use crate::persist::admin::{fbans, federations};
use crate::tg::admin_helpers::{
    create_federation, fban_user, fstat, get_fed, get_feds, is_fedadmin, is_fedmember, join_fed,
    subfed, update_fed,
};
use crate::tg::command::{Cmd, Context, TextArgs};
use crate::tg::markdown::MarkupType;
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::{GetUser, Username};
use crate::util::error::{BotError, Result};
use crate::{metadata::metadata, util::string::Speak};

use itertools::Itertools;
use macros::entity_fmt;
use sea_orm_migration::MigrationTrait;
use uuid::Uuid;

metadata!("Federations",
    r#"
    Federated bans are a way to maintain subscribable lists of banned users. Federations
    store lists of banned users and groups can subscribe to them to autoban all banned users
    in that federation.  

    Each federation has an owner, and a number of admins, all of which are cable of issuing fbans
    in that federation. Federations can subscribe to other federations to receive their bans \(but not 
    their actual ban list \) 
    "#,
    { command = "fban", help = "Bans a user in the current chat's federation" },
    { command = "joinfed", help = "Joins a chat to a federation. Only one fed per chat" },
    { command = "newfed", help = "Create a new federation with yourself as the owner" },
    { command = "myfeds", help = "Get a list of feds you are either the owner or admin of" },
    { command = "fpromote", help = "Promote another user as fedadmin. They need to click the message sent to confirm the promotion" },
    { command = "unfban", help = "Unban a user in the current chat's federation" },
    { command = "renamefed", help = "Rename your federation" },
    { command = "subfed", help = "Usage: subfed \\<uuid\\>: subscribes your federation to a new fed's id" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn fban(ctx: &Context) -> Result<()> {
    ctx.action_message(|ctx, user, args| async move {
        if let Some(user) = user.get_cached_user().await? {
            let chat = ctx.try_get()?.chat;
            if let Some(fed) = is_fedmember(chat.get_id()).await? {
                if is_fedadmin(user.get_id(), &fed).await?
                    || ctx.check_permissions(|p| p.is_support).await.is_ok()
                {
                    let mut model = fbans::Model::new(&user, fed);
                    model.reason = args
                        .map(|v| v.text.trim().to_owned())
                        .map(|v| (!v.is_empty()).then(|| v))
                        .flatten();
                    fban_user(model, &user).await?;
                    ctx.reply("Successfully fbanned").await?;
                } else {
                    ctx.reply("Permission denied, user is not a fedadmin")
                        .await?;
                }
            } else {
                ctx.reply("this chat is not in a federation").await?;
            }
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await?;
    Ok(())
}

async fn create_federation_cmd<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return Err(BotError::speak(
            "Anonymous channels can't own feds",
            message.get_chat().get_id(),
        ));
    }
    if let Some(from) = message.get_from() {
        let fedname = args.text.to_owned();
        let fed = federations::Model::new(from.get_id(), fedname);
        let s = format!("Created fed {}", fed.fed_id);
        create_federation(ctx, fed).await?;
        ctx.reply(s).await?;
    }
    Ok(())
}

async fn join_fed_cmd<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let fed = Uuid::parse_str(args.text)?;
    let chat = ctx.try_get()?.chat;
    join_fed(chat, &fed).await?;
    ctx.reply(format!("Joined fed {}", fed.to_string())).await?;
    Ok(())
}

async fn myfeds(ctx: &Context) -> Result<()> {
    if let Some(user) = ctx.message()?.get_from() {
        let feds = get_feds(user.get_id()).await?;
        let msg = feds
            .into_iter()
            .map(|f| {
                if f.owner == user.get_id() {
                    format!(
                        "You are the owner of fed {}, with id {}",
                        f.fed_name,
                        f.fed_id.to_string()
                    )
                } else {
                    format!(
                        "You are admin of fed {} with id {}",
                        f.fed_name,
                        f.fed_id.to_string()
                    )
                }
            })
            .join("\n");
        ctx.reply(format!(
            "Feds for user {}:\n{}",
            user.name_humanreadable(),
            msg
        ))
        .await?;
    }
    Ok(())
}

pub async fn unfban(ctx: &Context) -> Result<()> {
    ctx.action_message(|ctx, user, _| async move {
        if let Some(fed) = is_fedmember(ctx.try_get()?.chat.get_id()).await? {
            if is_fedadmin(user, &fed).await?
                || ctx.check_permissions(|p| p.is_support).await.is_ok()
            {
                ctx.unfban(user, &fed).await?;
            } else {
                ctx.reply("You need to be fedamin to unfban").await?;
            }
        } else {
            ctx.reply("This chat is not a member of a fed").await?;
        }
        Ok(())
    })
    .await?;
    Ok(())
}

async fn rename_fed<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    if let Some(owner) = ctx.message()?.get_from() {
        let fed = update_fed(owner.get_id(), args.text.to_owned())
            .await?
            .fed_id;
        ctx.reply(format!("Renamed fed {} to {}", fed.to_string(), args.text))
            .await?;
    }
    Ok(())
}

async fn subfed_cmd<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    let chat = ctx.try_get()?.chat.get_id();
    if let Some(user) = ctx.message()?.get_from() {
        let sub = Uuid::parse_str(args.text)?;
        let fed = get_fed(user.get_id())
            .await?
            .ok_or_else(|| BotError::speak("You currently do not have a fed", chat))?;
        subfed(&fed.fed_id, &sub).await?;
        ctx.reply(format!(
            "Successfully subscribed fed {} to {}",
            fed.fed_id, sub
        ))
        .await?;
    }
    Ok(())
}

async fn fstat_cmd(ctx: &Context) -> Result<()> {
    ctx.action_message(|ctx, user, _| async move {
        let lang = ctx.try_get()?.lang;
        let chat = ctx.try_get()?.chat.get_id();
        let v = fstat(user)
            .await?
            .map(|(fban, fed)| {
                format!(
                    "Banned in {} with reason \"{}\"",
                    fed.fed_id,
                    fban.reason
                        .as_ref()
                        .map(|v| v.as_str())
                        .unwrap_or("No reason")
                )
            })
            .join("\n");
        let v = MarkupType::Text.text(&v);
        ctx.reply_fmt(entity_fmt!(lang, chat, "fstat", user.mention().await?, v))
            .await?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, ref args, .. }) = ctx.cmd() {
        match cmd {
            "fban" => fban(ctx).await,
            "joinfed" => join_fed_cmd(ctx, args).await,
            "newfed" => create_federation_cmd(ctx, args).await,
            "myfeds" => myfeds(ctx).await,
            "fpromote" => ctx.fpromote().await,
            "unfban" => unfban(ctx).await,
            "renamefed" => rename_fed(ctx, args).await,
            "subfed" => subfed_cmd(ctx, args).await,
            "fstat" => fstat_cmd(ctx).await,
            _ => Ok(()),
        }?;
    }

    Ok(())
}
