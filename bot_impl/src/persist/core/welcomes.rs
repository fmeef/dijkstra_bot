use crate::persist::core::media::*;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "welcome")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat: i64,
    #[sea_orm(column_type = "Text")]
    pub text: Option<String>,
    pub media_id: Option<String>,
    pub media_type: Option<MediaType>,
    #[sea_orm(column_type = "Text")]
    pub goodbye_text: Option<String>,
    pub goodbye_media_id: Option<String>,
    pub goodbye_media_type: Option<MediaType>,
    #[sea_orm(default = false)]
    pub enabled: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl Related<super::welcomes::Entity> for Entity {
    fn to() -> RelationDef {
        panic!("no relations")
    }
}

impl ActiveModelBehavior for ActiveModel {}
