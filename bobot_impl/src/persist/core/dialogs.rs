use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dialogs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub chat_id: i64,
    pub last_activity: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::chat_members::Entity")]
    ChatMembers,
}

impl Related<super::chat_members::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChatMembers.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
