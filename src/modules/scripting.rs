use crate::metadata::metadata;
use crate::tg::admin_helpers::ActionMessage;
use crate::tg::command::{Cmd, Context};
use crate::tg::markdown::{Escape, MarkupType};
use crate::util::error::{Fail, Result, SpeakErr};
use crate::util::scripting::{ManagedRhai, RHAI_ENGINE};
use crate::util::string::Speak;
use macros::{entity_fmt, update_handler};
use rhai::Dynamic;

metadata!(
    "Scripting",
    r#"
    Automate group management functionality like never before with snippets of code!

    Scripts are written in a language called Rhai and can query telegram types, allowing for
    expressing complex moderation actions that tightly integrate with existing modules like
    blocklists.

    For more information on rhai, please visit https://rhai.rs/book. Most rhai feaures listed there
    will work in this bot!

    [__eval command:]\n
    the /eval command is useful for exploring the telegram api available to rhai scripts
    Eval takes an "anonymous function" as a parameter, which is rhai works like
    [`rust`
    |argument| {\n
      //do some things \n
    }
    ]\n
    The brackets can be omitted if the function body is a single expression
    [`rust`
    //do some things in a single expression\n
    |argument| argument.do_thing
    ]

    [__working with telegram types:]\n
    Print information about the current message. Useful for figuring out what fields you can access
    [`rust`
    |m| m.from.value.to_debug()\n
    // User { id: 208121891, is_bot: false, first_name: "Lina Torvalds: Heartbreaker", last_name: Some("(躺平)"), username: Some("emogamer95"), language_code: Some("en"), is_premium: Some(true), added_to_attachment_menu: None, can_join_groups: None, can_read_all_group_messages: None, supports_inline_queries: None, can_connect_to_business: None }
    ]

    Access a field. Option fields may be null, and have the [`.value] and [`.enum_type] getters
    [`rust`
    // m.from is not null\n
    //\n
    //core::option::Option<botapi::gen_types::User>\n
    |m| m.from\n
    // Some\n
    |m| m.from.enum_type\n
    // false\n
    |m| m.from.value == ()\n

    // m.from.value.can_connect_to_business is null\n
    //\n
    // None\n
    |m| m.from.value.can_connect_to_business.enum_type\n
    // true\n
    |m| m.from.value.can_connect_to_business.value == ()\n
    ]

    [__helper functions:]\n
    Text processing functionality is provided via helper functions. Current there is just [`glob]
    which allows text globbing similar to how the blocklist module works.
    [`rust`
    |m| glob("*coin*", m.from.value.username) // ban some coin scammers. Returns true if username has "coin"
    ]

    [__supported modules:]\n
    Currently the [*blocklists] module has alpha quality support for scripting in blocklists.
    for more information please see /help blocklists"#,
    {command = "eval", help = "Evaluates a test script using the current message as a parameter"}
);

async fn map_script(ctx: &Context) -> Result<()> {
    ctx.action_message(|ctx, am, args| async move {
        let args = args.ok_or_else(|| ctx.fail_err("missing arg"))?;
        let text = args.text.to_owned();
        let message = match am {
            ActionMessage::Me(m) => m,
            ActionMessage::Reply(m) => m,
        };
        log::info!("{}", text);
        let res: Dynamic = ManagedRhai::new_mapper(text, &RHAI_ENGINE, (message.clone(),))
            .post()
            .await
            .speak_err(ctx, |e| format!("Failed to compile: {}", e))
            .await?;
        let res = res.to_string();
        let res = if res.trim().is_empty() {
            "()".to_owned()
        } else {
            res
        };
        log::info!("{res}");
        let res = MarkupType::BlockQuote.text(res.escape(false));

        ctx.reply_fmt(entity_fmt!(ctx, "empty", res)).await?;
        Ok(())
    })
    .await
}

#[update_handler]
pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        if cmd == "eval" {
            map_script(ctx).await?;
        };
    }
    Ok(())
}
