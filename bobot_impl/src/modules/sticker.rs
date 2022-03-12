use crate::tg::client::TgClient;
use sea_schema::migration::MigrationTrait;
pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

pub async fn handle_update(_client: TgClient, _update: &grammers_client::types::update::Update) {}
