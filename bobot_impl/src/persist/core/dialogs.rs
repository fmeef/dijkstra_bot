use botapi::gen_types::Chat;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use crate::persist::admin::actions::ActionType;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dialogs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub chat_id: i64,
    #[sea_orm(default = crate::util::string::Lang::En)]
    pub language: crate::util::string::Lang,
    pub chat_type: String,
    pub warn_limit: i32,
    pub action_type: ActionType,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl Related<super::chat_members::Entity> for Entity {
    fn to() -> RelationDef {
        panic!("no relations")
    }
}

impl Model {
    pub fn from_chat(chat: &Chat) -> Self {
        Self {
            chat_id: chat.get_id(),
            language: crate::util::string::Lang::En,
            chat_type: chat.get_tg_type().into_owned(),
            warn_limit: 3,
            action_type: ActionType::Mute,
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}
