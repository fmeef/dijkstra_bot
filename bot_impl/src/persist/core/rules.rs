//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::media::MediaType;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "rules")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat_id: i64,
    #[sea_orm(column_type = "Text")]
    pub text: Option<String>,
    pub media_id: Option<String>,
    pub media_type: MediaType,
    #[sea_orm(default_value = false)]
    pub private: bool,
    #[sea_orm(column_type = "Text", default_value = "Rules")]
    pub button_name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
