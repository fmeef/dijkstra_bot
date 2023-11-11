use crate::metadata::metadata;
use crate::tg::command::{Cmd, Context};
use crate::util::error::Result;
use crate::util::string::Speak;

use super::all_export;

metadata!("Import/Export",
    r#"
    Import and export data from select modules in a format compatible with a certain feminine
    flower-based bot on telegram. 
    "#,
    { command = "import", help = "Import data for the current chat" },
    { command = "export", help = "Export data for the current chat"}
);

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, message, .. }) = ctx.cmd() {
        // log::info!("piracy command {}", cmd);
        match cmd {
            //            "crash" => TG.client().close().await?,
            "export" => {
                let v = all_export(message.get_chat().get_id()).await?;
                ctx.reply(serde_json::to_string_pretty(&v)?).await?;
            }
            _ => (),
        };
    }

    Ok(())
}
