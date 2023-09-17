use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "entitylist")]
pub struct Model {
    #[sea_orm(primary_key, autoincrement = true)]
    pub id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::persist::core::messageentity::Entity")]
    Entities,
    #[sea_orm(has_many = "crate::persist::core::button::Entity")]
    Buttons,
    #[sea_orm(
        belongs_to = "crate::persist::core::button::Entity",
        from = "Column::Id",
        to = "crate::persist::core::button::Column::OwnerId"
    )]
    ButtonsRev,
    #[sea_orm(
        belongs_to = "crate::persist::core::messageentity::Entity",
        from = "Column::Id",
        to = "crate::persist::core::messageentity::Column::OwnerId"
    )]
    EntitiesRev,
}

impl Related<crate::persist::core::messageentity::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Entities.def()
    }
}

impl Related<Entity> for crate::persist::core::messageentity::Entity {
    fn to() -> RelationDef {
        Relation::EntitiesRev.def().rev()
    }
}

impl Related<crate::persist::core::button::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Buttons.def()
    }
}

impl Related<Entity> for crate::persist::core::button::Entity {
    fn to() -> RelationDef {
        Relation::ButtonsRev.def().rev()
    }
}

impl ActiveModelBehavior for ActiveModel {}
