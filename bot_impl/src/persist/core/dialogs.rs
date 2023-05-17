//! ORM type for storing metadata on conversations
//! conversations being DMs, channels, and supergroups

use botapi::gen_types::Chat;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{persist::admin::actions::ActionType, statics::TG, util::error::BotError};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dialogs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub chat_id: i64,
    #[sea_orm(default = crate::util::string::Lang::En)]
    pub language: crate::util::string::Lang,
    pub chat_type: String,
    pub warn_limit: i32,
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
    pub warn_time: Option<i64>,
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
    pub async fn from_chat(chat: &Chat) -> crate::util::error::Result<Self> {
        let chat = TG.client.get_chat(chat.get_id()).await?;
        let permissions = chat
            .get_permissions()
            .ok_or_else(|| BotError::speak("failed to get chat permissions", chat.get_id()))?;

        let res = Self {
            chat_id: chat.get_id(),
            language: crate::util::string::Lang::En,
            chat_type: chat.get_tg_type().into_owned(),
            warn_limit: 3,
            action_type: ActionType::Mute,
            warn_time: None,
            can_send_messages: permissions.get_can_send_messages().unwrap_or(true),
            can_send_audio: permissions.get_can_send_audios().unwrap_or(true),
            can_send_video: permissions.get_can_send_videos().unwrap_or(true),
            can_send_photo: permissions.get_can_send_photos().unwrap_or(true),
            can_send_document: permissions.get_can_send_documents().unwrap_or(true),
            can_send_video_note: permissions.get_can_send_video_notes().unwrap_or(true),
            can_send_voice_note: permissions.get_can_send_voice_notes().unwrap_or(true),
            can_send_poll: permissions.get_can_send_polls().unwrap_or(true),
            can_send_other: permissions.get_can_send_other_messages().unwrap_or(true),
        };
        Ok(res)
    }
}

impl ActiveModelBehavior for ActiveModel {}
