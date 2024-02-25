//! ORM type for storing metadata on conversations
//! conversations being DMs, channels, and supergroups

use crate::tg::admin_helpers::is_dm;
use crate::util::error::Fail;
use crate::{persist::admin::actions::ActionType, statics::TG};
use botapi::gen_types::{Chat, ChatPermissionsBuilder};
use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue::{NotSet, Set};
use serde::{Deserialize, Serialize};

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
    pub federation: Option<Uuid>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::persist::admin::federations::Entity",
        from = "Column::Federation",
        to = "crate::persist::admin::federations::Column::FedId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Federation,
    #[sea_orm(
        belongs_to = "crate::persist::admin::fbans::Entity",
        from = "Column::Federation",
        to = "crate::persist::admin::fbans::Column::Federation",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Fbans,
}

impl Related<crate::persist::admin::federations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Federation.def()
    }
}

impl Related<crate::persist::admin::fbans::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Fbans.def()
    }
}

impl Model {
    pub async fn from_chat(chat: &Chat) -> crate::util::error::Result<ActiveModel> {
        let chat = TG.client.get_chat(chat.get_id()).await?;
        let def = &ChatPermissionsBuilder::new().build();
        let permissions = if is_dm(&chat) {
            &def
        } else {
            chat.get_permissions()
                .ok_or_else(|| chat.fail_err("failed to get chat permissions"))?
        };
        let res = ActiveModel {
            chat_id: Set(chat.get_id()),
            language: NotSet,
            chat_type: Set(chat.get_tg_type().to_owned()),
            warn_limit: NotSet,
            action_type: NotSet,
            warn_time: NotSet,
            can_send_messages: Set(permissions.get_can_send_messages().unwrap_or(true)),
            can_send_audio: Set(permissions.get_can_send_audios().unwrap_or(true)),
            can_send_video: Set(permissions.get_can_send_videos().unwrap_or(true)),
            can_send_photo: Set(permissions.get_can_send_photos().unwrap_or(true)),
            can_send_document: Set(permissions.get_can_send_documents().unwrap_or(true)),
            can_send_video_note: Set(permissions.get_can_send_video_notes().unwrap_or(true)),
            can_send_voice_note: Set(permissions.get_can_send_voice_notes().unwrap_or(true)),
            can_send_poll: Set(permissions.get_can_send_polls().unwrap_or(true)),
            can_send_other: Set(permissions.get_can_send_other_messages().unwrap_or(true)),
            federation: NotSet,
        };
        Ok(res)
    }
}

impl ActiveModelBehavior for ActiveModel {}
