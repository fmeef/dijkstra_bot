//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::media::MediaType;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "taint")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub media_id: String,
    #[sea_orm(unique)]
    pub scope: String,
    #[sea_orm(unique)]
    pub chat: i64,
    pub media_type: MediaType,
    pub notes: Option<String>,
    pub details: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
