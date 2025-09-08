use crate::tg::command::Context;
use crate::util::error::Result;

use crate::metadata::metadata;
use macros::update_handler;

metadata!("Clean Service",
    r#"Remove service messages in your group"#,
    { command = "addblocklist", help = "\\<trigger\\> \\<reply\\> {action}: Add a blocklist" },
    { command = "blocklist", help = "List all blocklists" },
    { command = "rmblocklist", help = "Stop a blocklist by trigger" },
    { command = "rmallblocklists", help = "Stop all blocklists" },
    { command = "scriptblocklist", help = "Adds a rhai script as a blocklist with a provided name" },
    { command = "rmscriptblocklist", help = "Moves a script blocklist by name"}
);

#[update_handler]
pub async fn handle_update<'a>(_: &Context) -> Result<()> {
    // handle_command(cmd).await?;

    //test
    Ok(())
}
