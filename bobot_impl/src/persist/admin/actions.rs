use crate::util::error::BotError;
use chrono::Utc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum ActionType {
    #[sea_orm(num_value = 1)]
    Mute,
    #[sea_orm(num_value = 2)]
    Ban,
    #[sea_orm(num_value = 3)]
    Shame,
    #[sea_orm(num_value = 4)]
    Warn,
    #[sea_orm(num_value = 5)]
    Delete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "actions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub user_id: i64,
    #[sea_orm(primary_key)]
    pub chat_id: i64,
    #[sea_orm(default = true)]
    pub pending: bool,
    #[sea_orm(default = false)]
    pub is_banned: bool,
    #[sea_orm(default = true)]
    pub can_send_messages: bool,
    #[sea_orm(default = true)]
    pub can_send_audio: bool,
    #[sea_orm(default = true)]
    pub can_send_video: bool,
    #[sea_orm(default = true)]
    pub can_send_photo: bool,
    #[sea_orm(default = true)]
    pub can_send_document: bool,
    #[sea_orm(default = true)]
    pub can_send_voice_note: bool,
    #[sea_orm(default = true)]
    pub can_send_video_note: bool,
    #[sea_orm(default = true)]
    pub can_send_poll: bool,
    #[sea_orm(default = true)]
    pub can_send_other: bool,
    pub action: Option<ActionType>,
    pub expires: Option<chrono::DateTime<Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl Related<super::actions::Entity> for Entity {
    fn to() -> RelationDef {
        panic!("no relations")
    }
}

impl ActionType {
    pub fn from_str<T: AsRef<str>>(s: T, chat: i64) -> crate::util::error::Result<Self> {
        Self::from_str_err(s.as_ref(), || {
            BotError::speak(format!("Invalid action {}", s.as_ref()), chat)
        })
    }

    pub fn get_name(&self) -> &str {
        match self {
            ActionType::Mute => "mute",
            ActionType::Ban => "ban",
            ActionType::Shame => "shame",
            ActionType::Warn => "warn",
            ActionType::Delete => "delete",
        }
    }

    pub fn from_str_err<T, F>(s: T, err: F) -> crate::util::error::Result<Self>
    where
        F: FnOnce() -> BotError,
        T: AsRef<str>,
    {
        match s.as_ref() {
            "mute" => Ok(ActionType::Mute),
            "ban" => Ok(ActionType::Ban),
            "warn" => Ok(ActionType::Warn),
            "shame" => Ok(ActionType::Warn),
            "delete" => Ok(ActionType::Delete),
            _ => Err(err()),
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}
