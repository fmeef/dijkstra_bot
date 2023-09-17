//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use botapi::gen_types::{InlineKeyboardButton, InlineKeyboardButtonBuilder};
use sea_orm::{entity::prelude::*, ActiveValue};
use serde::{Deserialize, Serialize};
use ActiveValue::Set;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "button")]
pub struct Model {
    pub button_text: String,
    pub callback_data: Option<String>,
    pub button_url: Option<String>,
    #[sea_orm(primary_key)]
    pub owner_id: i64,
    #[sea_orm(primary_key)]
    pub pos_x: u32,
    #[sea_orm(primary_key)]
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

    pub fn from_button(
        pos_x: u32,
        pos_y: u32,
        button: &InlineKeyboardButton,
        owner_id: i64,
    ) -> ActiveModel {
        ActiveModel {
            pos_x: Set(pos_x),
            pos_y: Set(pos_y),
            button_text: Set(button.get_text().into_owned()),
            button_url: Set(button.get_url().map(|v| v.into_owned())),
            owner_id: Set(owner_id),
            callback_data: Set(button.get_callback_data().map(|v| v.into_owned())),
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
