use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "fedadmins")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub federation: Uuid,
    #[sea_orm(primary_key)]
    pub user: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::federations::Entity",
        from = "Column::Federation",
        to = "super::federations::Column::FedId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Federations,

    #[sea_orm(
        belongs_to = "crate::persist::core::users::Entity",
        from = "Column::User",
        to = "crate::persist::core::users::Column::UserId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    User,
}

impl Related<super::federations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Federations.def()
    }
}

impl Related<crate::persist::core::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
