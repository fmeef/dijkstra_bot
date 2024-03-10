use crate::persist::admin::{fbans, federations};
use crate::persist::core::users;
use crate::statics::{DB, TG};
use crate::tg::admin_helpers::{FileGetter, StrOption};
use crate::tg::command::{Cmd, Context, TextArgs};
use crate::tg::federations::{
    create_federation, fban_user, fstat, get_fed, get_feds, is_fedadmin, is_fedmember, join_fed,
    subfed, try_update_fban_cache, update_fed,
};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::{GetUser, Username};
use crate::util::error::{BotError, Fail, Result, SpeakErr};
use crate::util::string::should_ignore_chat;
use crate::{metadata::metadata, util::string::Speak};
use botapi::bot::Part;
use botapi::gen_types::{FileData, Message};
use itertools::Itertools;
use macros::{entity_fmt, lang_fmt, update_handler};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sea_query::OnConflict;
use serde::{Deserialize, Serialize};
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
    { command = "subfed", help = "Usage: subfed \\<uuid\\>: subscribes your federation to a new fed's id" },
    { command = "fedimport", help = "Import a list of fbans to your current federation using Rose bot's json format" },
    { command = "fedexport", help = "Export your federation's fbans in Rose bot's json format" }
);

async fn fban(ctx: &Context) -> Result<()> {
    if ctx.message()?.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonban"));
    }

    ctx.action_user(|ctx, user, args| async move {
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
                    let reason = model.reason.clone();
                    fban_user(model, &user).await?;
                    if let Some(reason) = reason {
                        ctx.reply_fmt(entity_fmt!(
                            ctx,
                            "fbanreason",
                            user.mention().await?,
                            fed.to_string(),
                            reason
                        ))
                        .await?;
                    } else {
                        ctx.reply_fmt(entity_fmt!(
                            ctx,
                            "fban",
                            user.mention().await?,
                            fed.to_string()
                        ))
                        .await?;
                    }
                } else {
                    ctx.reply(lang_fmt!(ctx, "notfedadmin", fed.to_string()))
                        .await?;
                }
            } else {
                ctx.reply(lang_fmt!(ctx, "notinfed", chat.name_humanreadable()))
                    .await?;
            }
        } else {
            ctx.reply(lang_fmt!(ctx, "usernotfound")).await?;
        }

        Ok(())
    })
    .await?;
    Ok(())
}

async fn create_federation_cmd<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "erranonchannelfed"));
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
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }

    ctx.check_permissions(|p| p.can_restrict_members).await?;
    let fed = Uuid::parse_str(args.text)?;
    let chat = ctx.try_get()?.chat;
    join_fed(chat, &fed).await?;
    ctx.reply(lang_fmt!(
        ctx,
        "joinfed",
        fed.to_string(),
        chat.name_humanreadable()
    ))
    .await?;
    Ok(())
}

async fn myfeds(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }

    if ctx.message()?.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonban"));
    }

    if let Some(user) = ctx.message()?.get_from() {
        let feds = get_feds(user.get_id()).await?;
        let msg = feds
            .into_iter()
            .map(|f| {
                if f.owner == user.get_id() {
                    lang_fmt!(ctx, "fedowner", f.fed_name, f.fed_id.to_string())
                } else {
                    lang_fmt!(ctx, "fedadmin", f.fed_name, f.fed_id.to_string())
                }
            })
            .join("\n");
        ctx.reply_fmt(entity_fmt!(ctx, "fedsforuser", user.mention().await?, msg))
            .await?;
    }
    Ok(())
}

pub async fn unfban(ctx: &Context) -> Result<()> {
    if ctx.message()?.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonban"));
    }
    ctx.action_user(|ctx, user, _| async move {
        if let Some(fed) = is_fedmember(ctx.try_get()?.chat.get_id()).await? {
            if is_fedadmin(user, &fed).await?
                || ctx.check_permissions(|p| p.is_support).await.is_ok()
            {
                ctx.unfban(user, &fed).await?;
            } else {
                ctx.reply(lang_fmt!(ctx, "unfbanperm")).await?;
            }
        } else {
            ctx.reply(lang_fmt!(ctx, "notfmember")).await?;
        }
        Ok(())
    })
    .await?;
    Ok(())
}

async fn rename_fed<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }

    if let Some(owner) = ctx.message()?.get_from() {
        let fed = update_fed(owner.get_id(), args.text.to_owned())
            .await?
            .fed_id;
        ctx.reply(lang_fmt!(ctx, "renamefed", fed.to_string(), args.text))
            .await?;
    }
    Ok(())
}

async fn subfed_cmd<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }

    let chat = ctx.try_get()?.chat.get_id();
    if let Some(user) = ctx.message()?.get_from() {
        let sub = Uuid::parse_str(args.text)?;
        let fed = get_fed(user.get_id()).await?.ok_or_else(|| {
            BotError::speak(
                "You currently do not have a fed",
                chat,
                Some(message.message_id),
            )
        })?;
        subfed(&fed.fed_id, &sub).await?;
        ctx.reply(lang_fmt!(ctx, "subscribefed", fed.fed_id, sub))
            .await?;
    }
    Ok(())
}

async fn fstat_cmd(ctx: &Context) -> Result<()> {
    ctx.action_user(|ctx, user, _| async move {
        let v = fstat(user)
            .await?
            .map(|(fban, fed)| {
                lang_fmt!(
                    ctx,
                    "fstatline",
                    fed.fed_id,
                    fban.reason
                        .as_ref()
                        .map(|v| v.as_str())
                        .unwrap_or("No reason")
                )
            })
            .join("\n");
        ctx.reply_fmt(entity_fmt!(ctx, "fstat", user.mention().await?, v))
            .await?;
        Ok(())
    })
    .await?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct FbanExportItem {
    pub user_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub reason: String,
}

async fn get_fban_list(fed: &Uuid) -> Result<Vec<FbanExportItem>> {
    let res = fbans::Entity::find()
        .filter(crate::persist::admin::fbans::Column::Federation.eq(*fed))
        .find_also_related(users::Entity)
        .all(*DB)
        .await?;
    Ok(res
        .into_iter()
        .filter_map(|(fban, user)| user.map(|user| (fban, user)))
        .map(|(fban, user)| FbanExportItem {
            user_id: user.user_id,
            first_name: user.first_name,
            last_name: user.last_name.unwrap_or_else(|| "".to_owned()),
            reason: fban.reason.unwrap_or_else(|| "".to_owned()),
        })
        .collect())
}

async fn set_fban_list(ctx: &Context, fed: &Uuid, message: &Message) -> Result<u64> {
    if let Some(document) = message.get_document() {
        let text = document.get_text().await?;
        let fb = serde_json::Deserializer::from_str(&text).into_iter::<FbanExportItem>();
        let res = fb.map_ok(|fb| {
            (
                users::ActiveModel {
                    user_id: Set(fb.user_id),
                    first_name: Set(fb.first_name),
                    last_name: Set(fb.last_name.none_if_empty()),
                    username: NotSet,
                    is_bot: NotSet,
                },
                fbans::ActiveModel {
                    fban_id: Set(Uuid::new_v4()),
                    federation: Set(*fed),
                    user: Set(fb.user_id),
                    user_name: NotSet,
                    reason: Set(fb.reason.none_if_empty()),
                },
                fb.user_id,
            )
        });

        let (user, fbs, ids): (Vec<_>, Vec<_>, Vec<_>) =
            itertools::process_results(res, |i| itertools::multiunzip(i))
                .speak_err(ctx, |e| format!("Failed to parse fban json: {}", e))
                .await?;
        users::Entity::insert_many(user)
            .on_conflict(
                OnConflict::column(users::Column::UserId) //SECURITY ALERT don't modify existing users
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(*DB)
            .await?;

        let res = fbans::Entity::insert_many(fbs)
            .on_conflict(
                OnConflict::column(fbans::Column::User)
                    .update_columns([fbans::Column::UserName, fbans::Column::Reason])
                    .to_owned(),
            )
            .exec_without_returning(*DB)
            .await?;
        for id in ids {
            try_update_fban_cache(id).await?;
        }

        Ok(res)
    } else {
        ctx.fail("Message is not a file")
    }
}

async fn import_fbans(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }
    if let Some(user) = message.get_from() {
        let user = user.get_id();
        ctx.action_message(|ctx, message, _| async move {
            if let Some(fed) = get_fed(user).await? {
                let res = set_fban_list(ctx, &fed.fed_id, message.message()).await?;
                ctx.reply(format!("Successfully imported {} fbans", res))
                    .await?;
            }

            Ok(())
        })
        .await?;
    }
    Ok(())
}

async fn export_fbans(ctx: &Context) -> Result<()> {
    let message = ctx.message()?;
    if message.get_sender_chat().is_some() {
        return ctx.fail(lang_fmt!(ctx, "anonfed"));
    }

    if let Some(user) = message.get_from() {
        if let Some(fed) = get_fed(user.get_id()).await? {
            let export = get_fban_list(&fed.fed_id).await?;
            let export = serde_json::to_string(&export)?;
            let bytes = FileData::Part(Part::text(export).file_name("export.json"));
            if !should_ignore_chat(message.get_chat().get_id()).await? {
                TG.client
                    .build_send_document(message.get_chat().get_id(), bytes)
                    .build()
                    .await?;
            }
        } else {
            return ctx.fail(lang_fmt!(ctx, "nofed"));
        }
    }
    Ok(())
}

#[update_handler]
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
            "fedexport" => export_fbans(ctx).await,
            "fedimport" => import_fbans(ctx).await,
            _ => Ok(()),
        }?;
    }

    Ok(())
}
