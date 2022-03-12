use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "stickers")]
pub struct Model {
    #[sea_orm(column_type = "Text")]
    #[sea_orm(primary_key)]
    pub unique_id: String,
    pub owner_id: i64,
    #[sea_orm(column_type = "Text")]
    pub file_id: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub chosen_name: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::tags::Entity")]
    Tags,
}

impl Related<super::tags::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tags.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
