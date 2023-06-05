use crate::statics::TG;
use crate::tg::admin_helpers::action_message;
use crate::tg::command::{Context, Entities};
use crate::tg::markdown::MarkupBuilder;
use crate::util::error::Result;
use crate::{metadata::metadata, util::string::Speak};
use botapi::gen_types::{Message, UpdateExt};
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

async fn get_id<'a>(message: &Message, entities: &Entities<'a>) -> Result<()> {
    action_message(message, entities, None, |message, user, _| {
        async move {
            let mut builder = MarkupBuilder::new();
            builder.code(user.to_string());
            let (text, entities) = builder.build();
            message
                .reply_fmt(
                    TG.client
                        .build_send_message(message.get_chat().get_id(), text)
                        .entities(entities),
                )
                .await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(())
}

pub async fn handle_update<'a>(_: &UpdateExt, ctx: &Option<Context<'a>>) -> Result<()> {
    if let Some(ctx) = ctx {
        if let Some((cmd, entities, _, message, _)) = ctx.cmd() {
            match cmd {
                "id" => get_id(message, entities).await?,
                _ => (),
            }
        }
    }
    Ok(())
}
