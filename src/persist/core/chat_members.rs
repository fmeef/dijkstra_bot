use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, Eq, Hash)]
#[sea_orm(table_name = "chat_members")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat_id: i64,
    #[sea_orm(primary_key)]
    pub user_id: i64,
    pub banned_by_me: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::dialogs::Entity",
        from = "Column::ChatId",
        to = "super::dialogs::Column::ChatId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Dialogs,
}

impl Related<super::dialogs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Dialogs.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
