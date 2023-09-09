use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "captcha_auth")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat: i64,
    #[sea_orm(primary_key)]
    pub user: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl Related<super::captchastate::Entity> for Entity {
    fn to() -> RelationDef {
        panic!("no relations")
    }
}

impl ActiveModelBehavior for ActiveModel {}
