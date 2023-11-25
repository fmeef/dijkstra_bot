use std::ops::Deref;

use botapi::gen_types::FileData;
use itertools::Itertools;
use reqwest::multipart::Part;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::metadata::metadata;
use crate::persist::core::taint;
use crate::statics::{DB, TG};
use crate::tg::admin_helpers::FileGetter;
use crate::tg::command::{Cmd, Context};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::Username;
use crate::util::error::Result;
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

async fn get_taint(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_manage_chat).await?;
    let message = ctx.message()?;
    let taints = taint::Entity::find()
        .filter(taint::Column::Chat.eq(message.get_chat().get_id()))
        // .group_by(taint::Column::Scope)
        .order_by_asc(taint::Column::Scope)
        .all(DB.deref())
        .await?;

    let m = taints.into_iter().group_by(|v| v.scope.clone());

    let m = m
        .into_iter()
        .map(|(scope, t)| {
            let contents = t
                .into_iter()
                .map(|t| {
                    let notes = t.notes.unwrap_or_else(|| "".to_owned());
                    let media = t.id;
                    format!("{} - {}", media, notes)
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

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, message, .. }) = ctx.cmd() {
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
                ctx.action_message_message(|ctx, message, _| async move {
                    if let Some(file) = message.get_document() {
                        let text = file.get_text().await?;
                        all_import(message.get_chat().get_id(), &text).await?;
                        ctx.reply(format!(
                            "Imported data for chat {}",
                            message.get_chat().name_humanreadable()
                        ))
                        .await?;
                    } else {
                        ctx.reply("Please select a json file").await?;
                    }
                    Ok(())
                })
                .await?;
            }
            "taint" => {
                get_taint(ctx).await?;
            }
            _ => (),
        };
    }

    Ok(())
}
