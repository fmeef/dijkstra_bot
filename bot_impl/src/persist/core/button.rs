//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "button")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub button_id: i64,
    pub button_text: String,
    pub callback_data: Option<String>,
    pub button_url: Option<String>,
    pub owner_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
