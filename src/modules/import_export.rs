use botapi::gen_types::{EReplyMarkup, FileData};
use convert_case::{Case, Casing};
use itertools::Itertools;
use macros::{lang_fmt, update_handler};
use reqwest::multipart::Part;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use std::collections::HashMap;
use uuid::Uuid;

use crate::metadata::metadata;
use crate::persist::core::taint;
use crate::statics::{DB, TG};
use crate::tg::admin_helpers::FileGetter;
use crate::tg::command::{Cmd, Context, TextArgs};
use crate::tg::dialog::ConversationState;
use crate::tg::markdown::EntityMessage;
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::Username;
use crate::util::error::{Fail, Result};
use crate::util::string::{should_ignore_chat, Speak};

use super::{all_export, all_import};

metadata!("Import/Export",
    r#"
    Import and export data from select modules in a format compatible with a certain feminine
    flower-based bot on telegram. 
    "#,
    { command = "import", help = "Import data for the current chat" },
    { command = "export", help = "Export data for the current chat"}
);

#[allow(dead_code)]
async fn get_taint<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    let message = ctx.message()?;
    let taints = if let Some(filter) = args.args.first() {
        let filter = format!("{}%", filter.get_text()); // need wildcard at the end because b-tree index
        log::info!("filtering {}", filter);
        taint::Entity::find()
            .filter(
                taint::Column::Chat
                    .eq(message.get_chat().get_id())
                    .and(taint::Column::Scope.like(filter)),
            )
            .order_by_asc(taint::Column::Scope)
            .all(*DB)
            .await?
    } else {
        taint::Entity::find()
            .filter(taint::Column::Chat.eq(message.get_chat().get_id()))
            .order_by_asc(taint::Column::Scope)
            .all(*DB)
            .await?
    };

    let m = taints.into_iter().group_by(|v| v.scope.clone());

    let m = m
        .into_iter()
        .map(|(scope, t)| {
            let contents = t
                .into_iter()
                .map(|t| {
                    let notes = t.notes.unwrap_or_else(|| "".to_owned());
                    let media = t.id;
                    format!("[`{}] - {}", media, notes)
                })
                .join("\n");
            format!("[*{}:]\n{}", scope, contents)
        })
        .join("\n");

    let m = format!(
        "Broken media for {} by module:\n\n{}",
        message.get_chat().name_humanreadable(),
        m
    );

    ctx.reply(m).await?;
    Ok(())
}

#[allow(dead_code)]
async fn get_taint_menu(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    let message = ctx.message()?;
    let taints = taint::Entity::find()
        .filter(taint::Column::Chat.eq(message.get_chat().get_id()))
        // .group_by(taint::Column::Scope)
        .order_by_asc(taint::Column::Scope)
        .all(*DB)
        .await?;

    let m: HashMap<&str, Vec<&taint::Model>> =
        taints.iter().fold(HashMap::new(), |mut acc, val| {
            let vec = acc.entry(val.scope.as_str()).or_insert_with(|| Vec::new());
            vec.push(&val);
            acc
        });
    let text = "Select a module to recover media from";
    let mut state = ConversationState::new_prefix(
        "import".to_owned(),
        text.to_owned(),
        message.get_chat().get_id(),
        message
            .get_from()
            .map(|u| u.get_id())
            .ok_or_else(|| message.fail_err("User does not exist"))?,
        "button",
    )?;

    let start = state.get_start()?.state_id;
    for (key, value) in m {
        let contents = value
            .into_iter()
            .map(|t| {
                let notes = t.notes.as_ref().map(|v| v.as_str()).unwrap_or("");
                let media = t.id;
                format!("[`{}] - {}", media, notes)
            })
            .join("\n");
        let s = state.add_state(contents);
        state.add_transition(start, s, key, &key.to_case(Case::Title));
        state.add_transition(s, start, "back", "Back");
    }

    let conversation = state.build();
    conversation.write_self().await?;

    ctx.reply_fmt(
        EntityMessage::from_text(message.get_chat().get_id(), text).reply_markup(
            EReplyMarkup::InlineKeyboardMarkup(conversation.get_current_markup(3).await?),
        ),
    )
    .await?;

    Ok(())
}

async fn update_taint<'a>(ctx: &Context, args: &TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let uuid = Uuid::parse_str(args.text)?;
    ctx.update_taint_id(uuid).await?;
    Ok(())
}

#[update_handler]
pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        message,
        ref args,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "export" => {
                ctx.check_permissions(|p| p.can_manage_chat).await?;
                if !should_ignore_chat(message.get_chat().get_id()).await? {
                    let v = all_export(message.get_chat().get_id()).await?;
                    let out = serde_json::to_string_pretty(&v)?;

                    let bytes = FileData::Part(Part::text(out).file_name("export.txt"));
                    TG.client
                        .build_send_document(message.get_chat().get_id(), bytes)
                        .build()
                        .await?;
                }
            }
            "import" => {
                ctx.check_permissions(|p| p.can_change_info.and(p.can_restrict_members))
                    .await?;
                ctx.action_message(|ctx, message, _| async move {
                    let message = message.message();
                    if let Some(file) = message.get_document() {
                        let text = file.get_text().await?;
                        all_import(message.get_chat().get_id(), &text).await?;
                        let taint = taint::Entity::find()
                            .filter(taint::Column::Chat.eq(message.get_chat().get_id()))
                            .count(*DB)
                            .await?;

                        if taint == 0 {
                            ctx.reply(lang_fmt!(
                                ctx,
                                "imported",
                                message.get_chat().name_humanreadable()
                            ))
                            .await?;
                        } else {
                            ctx.reply(lang_fmt!(
                                ctx,
                                "taintdetected",
                                message.get_chat().name_humanreadable()
                            ))
                            .await?;
                        }
                    } else {
                        ctx.reply("Please select a json file").await?;
                    }
                    Ok(())
                })
                .await?;
            }
            "taint" => {
                get_taint(ctx, args).await?;
            }
            "fixtaint" => {
                update_taint(ctx, args).await?;
            }
            _ => (),
        };
    }

    Ok(())
}
