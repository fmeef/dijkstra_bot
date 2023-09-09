use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use crate::util::error::BotError;

#[derive(
    EnumIter, DeriveActiveEnum, Serialize, Deserialize, Clone, PartialEq, Debug, DeriveIden,
)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum CaptchaType {
    #[sea_orm(num_value = 1)]
    Button,
    #[sea_orm(num_value = 2)]
    Text,
}

impl CaptchaType {
    pub fn from_str(text: &str, chat: i64) -> crate::util::error::Result<Self> {
        match text {
            "button" => Ok(CaptchaType::Button),
            "text" => Ok(CaptchaType::Text),
            _ => Err(BotError::speak("Invalid button type", chat)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "captcha")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub chat: i64,
    #[sea_orm(default = CaptchaType::Button)]
    pub captcha_type: CaptchaType,
    pub kick_time: Option<i64>,
    pub captcha_text: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl Related<super::captchastate::Entity> for Entity {
    fn to() -> RelationDef {
        panic!("no relations")
    }
}

impl ActiveModelBehavior for ActiveModel {}
