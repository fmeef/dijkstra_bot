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

impl ActiveModelBehavior for ActiveModel {}
