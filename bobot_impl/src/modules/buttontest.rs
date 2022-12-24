use botapi::gen_types::UpdateExt;
use sea_orm_migration::MigrationTrait;

use crate::metadata::metadata;

metadata!("Piracy detection",
   { command = "report", help = "Report a pirate for termination" }
);

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn handle_update(_update: &UpdateExt) {}
