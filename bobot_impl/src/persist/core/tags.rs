use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(column_type = "Text")]
    pub sticker_id: String,
    pub owner_id: i64,
    #[sea_orm(column_type = "Text")]
    pub tag: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::stickers::Entity",
        from = "Column::StickerId",
        to = "super::stickers::Column::OwnerId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Stickers,
}

impl Related<super::stickers::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Stickers.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
