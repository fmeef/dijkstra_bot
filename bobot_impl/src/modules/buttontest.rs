use anyhow::Result;
use botapi::gen_types::{Message, UpdateExt};
use sea_orm_migration::MigrationTrait;

use crate::metadata::metadata;

metadata!("Piracy detection",
   { command = "report", help = "Report a pirate for termination" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

#[allow(dead_code)]
async fn handle_command(_message: &Message) -> Result<()> {
    Ok(())
}

#[allow(dead_code)]
pub async fn handle_update(update: &UpdateExt) {
    match update {
        UpdateExt::Message(ref message) => {
            if let Err(err) = handle_command(message).await {
                log::error!("cry {}", err);
            }
        }
        _ => (),
    }
}
