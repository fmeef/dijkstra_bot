use crate::persist::admin::gbans;
use crate::tg::admin_helpers::gban_user;
use crate::tg::command::{Cmd, Context};
use crate::tg::permissions::IsGroupAdmin;
use crate::tg::user::GetUser;
use crate::util::error::Result;
use crate::{metadata::metadata, util::string::Speak};

use sea_orm_migration::MigrationTrait;

metadata!("Global Bans",
    r#"
    This is just a debugging module, it will be removed eventually. 
    "#,
    { command = "bun", help = "Report a pirate for termination" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn ungban(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.is_support).await?;
    ctx.action_message(|ctx, user, _| async move {
        if let Some(user) = user.get_cached_user().await? {
            ctx.ungban_user(user.get_id()).await?;
            ctx.reply("user ungbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await?;
    Ok(())
}
async fn gban(ctx: &Context) -> Result<()> {
    ctx.check_permissions(|p| p.is_support).await?;
    ctx.action_message(|ctx, user, _| async move {
        if let Some(user) = user.get_cached_user().await? {
            gban_user(gbans::Model::new(user.get_id()), user).await?;
            ctx.reply("user gbanned").await?;
        } else {
            ctx.reply("user not found").await?;
        }

        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, .. }) = ctx.cmd() {
        match cmd {
            "gban" => gban(ctx).await,
            "ungban" => ungban(ctx).await,
            _ => Ok(()),
        }?;
    }

    Ok(())
}
