//! ORM type for a "pending action" that will be applied the next time the user is seen.
//! This allows for bans or restrictions to be applied to a user that has not been interacted with
//! in 48 hours (a telegram limitation) or is not cached in redis and specified by username

use crate::util::error::BotError;
use chrono::Utc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, Debug)]
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
    pub pending: bool,
    pub is_banned: bool,
    pub can_send_messages: bool,
    pub can_send_audio: bool,
    pub can_send_video: bool,
    pub can_send_photo: bool,
    pub can_send_document: bool,
    pub can_send_voice_note: bool,
    pub can_send_video_note: bool,
    pub can_send_poll: bool,
    pub can_send_other: bool,
    pub action: Option<ActionType>,
    pub expires: Option<chrono::DateTime<Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

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

    pub fn get_severity(&self) -> u32 {
        match self {
            ActionType::Shame => 0,
            ActionType::Delete => 1,
            ActionType::Warn => 2,
            ActionType::Mute => 3,
            ActionType::Ban => 4,
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

impl PartialOrd for ActionType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.get_severity().partial_cmp(&other.get_severity())
    }
}

impl Ord for ActionType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_severity().cmp(&other.get_severity())
    }
}

impl PartialEq for ActionType {
    fn eq(&self, other: &Self) -> bool {
        self.get_severity() == other.get_severity()
    }
}

impl Eq for ActionType {}

impl ActiveModelBehavior for ActiveModel {}
