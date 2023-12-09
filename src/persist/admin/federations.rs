use sea_orm::{entity::prelude::*, FromJsonQueryResult};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, FromJsonQueryResult,
)]
#[sea_orm(table_name = "federations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub fed_id: Uuid,
    pub subscribed: Option<Uuid>,
    #[sea_orm(unique)]
    pub owner: i64,
    pub fed_name: String,
}

impl Model {
    pub fn new(owner: i64, fed_name: String) -> Self {
        Model {
            subscribed: None,
            fed_id: Uuid::new_v4(),
            owner,
            fed_name,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::federations::Entity",
        from = "Column::Subscribed",
        to = "super::federations::Column::FedId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Subscriptions,
    #[sea_orm(has_many = "super::fbans::Entity")]
    Fbans,
    #[sea_orm(has_many = "super::fedadmin::Entity")]
    Admin,
    #[sea_orm(has_many = "crate::persist::core::dialogs::Entity")]
    Dialogs,
}

impl Related<super::federations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Subscriptions.def()
    }
}

impl Related<crate::persist::core::dialogs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Dialogs.def()
    }
}

impl Related<super::fedadmin::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Admin.def()
    }
}

impl Related<super::fbans::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Fbans.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
