use crate::metadata::metadata;
use crate::persist::core::media::{get_media_type, MediaType, SendMediaReply};
use crate::persist::core::rules;
use crate::persist::redis::{default_cache_query, CachedQueryTrait, RedisCache};
use crate::statics::{CONFIG, DB};

use crate::tg::command::{handle_deep_link, Cmd, Context};
use crate::tg::markdown::rules_deeplink_key;
use crate::tg::permissions::IsGroupAdmin;
use crate::util::error::Result;
use crate::util::string::{Lang, Speak};

use chrono::Duration;
use futures::FutureExt;
use lazy_static::__Deref;

use macros::{lang_fmt, update_handler};
use sea_orm::EntityTrait;
use sea_query::OnConflict;

metadata!("Rules",
    r#"
    Set rules for your chat. Rules can be murkdown formatted text \(see /help formatting\)
    or images, video, stickers, etc. Rules can be accessed via formfilling using the \{rules\}
    tag in filters or notes. This will create a button attached to the message linking to the rules
    in dm.
    "#,
    { command = "setrules", help = "Sets the current rules for this chat" },
    { command = "rules", help = "Gets the rules in dm"}
);

fn rules_model(ctx: &Context) -> Result<rules::Model> {
    let message = ctx.message()?;
    let (text, media_id, media_type) = if let Some(message) = message.get_reply_to_message() {
        let (media_id, media_type) = get_media_type(message)?;

        (
            message.get_text().map(|t| t.to_owned()),
            media_id,
            media_type,
        )
    } else {
        let (media_id, media_type) = get_media_type(message)?;
        let text = ctx.cmd().map(|&Cmd { ref args, .. }| args.text.to_owned());
        (text, media_id, media_type)
    };

    let model = rules::Model {
        chat_id: message.get_chat().get_id(),
        private: false,
        text,
        media_id,
        media_type,
        button_name: "Rules".to_owned(),
    };
    Ok(model)
}

#[inline(always)]
fn get_rules_key(chat: i64) -> String {
    format!("rules:{}", chat)
}

async fn save_rule<'a>(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info).await?;
    let message = ctx.message()?;
    let key = get_rules_key(message.get_chat().get_id());
    let model = rules_model(ctx)?;
    rules::Entity::insert(model.cache(&key).await?)
        .on_conflict(
            OnConflict::column(rules::Column::ChatId)
                .update_columns([
                    rules::Column::Text,
                    rules::Column::MediaId,
                    rules::Column::MediaType,
                    rules::Column::Private,
                ])
                .to_owned(),
        )
        .exec(DB.deref())
        .await?;

    ctx.reply(lang_fmt!(ctx.try_get()?.lang, "saverules"))
        .await?;
    Ok(())
}

#[inline(always)]
fn default_rules(chat_id: i64, lang: &Lang) -> rules::Model {
    rules::Model {
        chat_id,
        media_id: None,
        media_type: MediaType::Text,
        private: false,
        text: Some(lang_fmt!(lang, "norules")),
        button_name: "Rules".to_owned(),
    }
}

async fn rules(ctx: &Context) -> Result<()> {
    ctx.reply(lang_fmt!(ctx.try_get()?.lang, "getrules"))
        .await?;
    Ok(())
}

async fn get_rule(chat_id: i64) -> Result<Option<rules::Model>> {
    let key = get_rules_key(chat_id);
    let rules = default_cache_query(
        |_, _| async move {
            let r = rules::Entity::find_by_id(chat_id).one(DB.deref()).await?;
            Ok(r)
        },
        Duration::seconds(CONFIG.timing.cache_timeout as i64),
    )
    .query(&key, &())
    .await?;
    Ok(rules)
}

#[update_handler]
pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "setrules" => save_rule(ctx).await,
            "rules" => rules(ctx).await,
            "start" => {
                let key: Option<i64> = handle_deep_link(ctx, |k| rules_deeplink_key(k)).await?;
                if let Some(chat_id) = key {
                    let rules = if let Some(rules) = get_rule(chat_id).await? {
                        rules
                    } else {
                        default_rules(chat_id, ctx.try_get()?.lang)
                    };

                    SendMediaReply::new(ctx, rules.media_type)
                        .button_callback(|_, _| async move { Ok(()) }.boxed())
                        .text(rules.text)
                        .media_id(rules.media_id)
                        .send_media_reply()
                        .await?;
                }
                Ok(())
            }
            _ => Ok(()),
        }?;
    }
    Ok(())
}
