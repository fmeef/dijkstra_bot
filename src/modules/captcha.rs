use crate::metadata::metadata;

use crate::persist::admin::captchastate::CaptchaType;
use crate::persist::redis::RedisStr;
use crate::statics::REDIS;

use crate::tg::command::{ArgSlice, Cmd, Context, TextArgs};
use crate::tg::greetings::{get_callback_key, get_captcha_auth_key, send_captcha};
use crate::tg::permissions::*;
use crate::tg::user::Username;
use crate::util::error::Fail;
use crate::util::error::Result;
use crate::util::string::Speak;
use base64::engine::general_purpose;
use base64::Engine;
use botapi::gen_types::{Chat, User};
use macros::{lang_fmt, update_handler};
use redis::AsyncCommands;
use sea_orm_migration::MigrationName;
use uuid::Uuid;

metadata!("Captcha",
    r#"
       Set a captcha in the group to keep bots out. Supports two security levels, text and button.
    "#,
    { command = "captcha", help = "Enabled or disables captcha. Usage: /captcha \\<on/off\\>" },
    { command = "captchamode", help = "Sets the captcha mode to either button or text"},
    { command = "captchakick", help = "Sets the timeout for removing users who haven't solved the captcha. off to disable"}

);

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20230214_000001_create_captcha"
    }
}

async fn captchakick_cmd<'a>(ctx: &Context, args: &'a TextArgs<'a>) -> Result<()> {
    ctx.check_permissions(|p| p.can_change_info.and(p.can_restrict_members))
        .await?;
    let message = ctx.message()?;
    match args.as_slice() {
        ArgSlice { text: "off", .. } => {
            ctx.captchakick(None).await?;
            message.reply(lang_fmt!(ctx, "enablekick")).await?;
        }
        slice => {
            if let Some(time) = ctx.parse_duration(&Some(slice))? {
                ctx.captchakick(Some(time.num_seconds())).await?;
                message.reply(lang_fmt!(ctx, "disablekick")).await?;
            } else {
                message.reply(lang_fmt!(ctx, "invalidargument")).await?;
            }
        }
    }
    Ok(())
}

async fn handle_command<'a>(ctx: &Context) -> Result<()> {
    if let Some(&Cmd {
        cmd,
        ref args,
        message,
        ..
    }) = ctx.cmd()
    {
        match cmd {
            "captchakick" => {
                captchakick_cmd(ctx, args).await?;
            }
            "captchamode" => {
                let t = CaptchaType::from_str(
                    args.args.first().map(|a| a.get_text()).unwrap_or(""),
                    message.get_chat().get_id(),
                    message.message_id,
                )?;
                ctx.captchamode(t).await?;
            }
            "captcha" => match args.args.first().map(|a| a.get_text()) {
                Some("on") => ctx.enable_captcha().await?,
                Some("off") => ctx.disable_captcha().await?,
                _ => return ctx.fail("Invalid argument, use on or off"),
            },
            "start" => {
                if let (Some(user), Some(u)) =
                    (message.get_from(), args.args.first().map(|a| a.get_text()))
                {
                    let base = general_purpose::URL_SAFE_NO_PAD.decode(u)?;
                    let base = Uuid::from_slice(base.as_slice())?;
                    let key = get_callback_key(&base.to_string());
                    let base: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
                    if let Some(base) = base {
                        let (cchat, cuser): (Chat, User) = base.get()?;
                        let key = get_captcha_auth_key(cuser.get_id(), cchat.get_id());
                        if REDIS.sq(|q| q.exists(&key)).await? {
                            log::info!("chat {}", cchat.name_humanreadable());
                            if cuser.get_id() == user.get_id() {
                                send_captcha(message, cchat, ctx).await?;
                            }
                        } else {
                            ctx.reply(lang_fmt!(ctx, "captchanotauthorized")).await?;
                        }
                    }
                }
            }
            _ => (),
        };
    }
    Ok(())
}

#[update_handler]
pub async fn handle_update<'a>(cmd: &Context) -> Result<()> {
    handle_command(cmd).await?;
    Ok(())
}
