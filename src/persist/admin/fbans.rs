use botapi::gen_types::User;
use sea_orm::{entity::prelude::*, FromJsonQueryResult};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, FromJsonQueryResult,
)]
#[sea_orm(table_name = "fbans")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub fban_id: Uuid,
    pub federation: Uuid,
    pub user: i64,
    pub user_name: Option<String>,
    pub reason: Option<String>,
}

impl Model {
    pub fn new(user: &User, federation: Uuid) -> Self {
        Model {
            federation,
            fban_id: Uuid::new_v4(),
            user_name: user.get_username().map(|v| v.to_owned()),
            user: user.get_id(),
            reason: None,
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
        belongs_to = "super::federations::Entity",
        from = "Column::Federation",
        to = "super::federations::Column::FedId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Federation,

    #[sea_orm(
        belongs_to = "crate::persist::core::users::Entity",
        from = "Column::User",
        to = "crate::persist::core::users::Column::UserId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    User,
    #[sea_orm(has_many = "crate::persist::core::dialogs::Entity")]
    Dialogs,
}

impl Related<super::federations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Federation.def()
    }
}

impl Related<crate::persist::core::dialogs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Dialogs.def()
    }
}

impl Related<crate::persist::core::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
