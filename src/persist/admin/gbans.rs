use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "gbans")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub user: i64,
    pub id: Uuid,
    pub reason: Option<String>,
}

impl Model {
    pub fn new(user: i64) -> Self {
        Model {
            id: Uuid::new_v4(),
            reason: None,
            user,
        }
    }

    pub fn reason(mut self, reason: String) -> Self {
        self.reason = Some(reason);
        self
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::persist::core::users::Entity",
        from = "Column::User",
        to = "crate::persist::core::users::Column::UserId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    User,
}

impl Related<crate::persist::core::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
