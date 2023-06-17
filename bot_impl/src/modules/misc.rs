use crate::statics::TG;
use crate::tg::command::Context;
use crate::tg::markdown::MarkupBuilder;
use crate::util::error::Result;
use crate::{metadata::metadata, util::string::Speak};

use futures::FutureExt;
use sea_orm_migration::MigrationTrait;

metadata!("Misc",
   r#"
    Random helper functions to make your life easier.
    "#,
   { command = "id", help = "Gets the id for a user" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

async fn get_id(ctx: &Context) -> Result<()> {
    ctx.action_message(|ctx, user, _| {
        async move {
            if let Some(chat) = ctx.chat() {
                let mut builder = MarkupBuilder::new();

                builder.code(user.to_string());
                let (text, entities) = builder.build();
                ctx.message()?
                    .reply_fmt(
                        TG.client
                            .build_send_message(chat.get_id(), text)
                            .entities(entities),
                    )
                    .await?;
            }
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some((cmd, _, _, _, _)) = ctx.cmd() {
        match cmd {
            "id" => get_id(ctx).await?,
            _ => (),
        }
    }

    Ok(())
}
