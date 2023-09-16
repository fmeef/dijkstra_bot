//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use botapi::gen_types::{InlineKeyboardButton, InlineKeyboardButtonBuilder};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "button")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub button_id: i64,
    pub button_text: String,
    pub callback_data: Option<String>,
    pub button_url: Option<String>,
    pub owner_id: i64,
    pub pos_x: u32,
    pub pos_y: u32,
}

impl Model {
    pub fn to_button(self) -> InlineKeyboardButton {
        let mut b = InlineKeyboardButtonBuilder::new(self.button_text);

        if let Some(text) = self.callback_data {
            b = b.set_callback_data(text);
        }

        if let Some(url) = self.button_url {
            b = b.set_url(url);
        }
        b.build()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
