//! ORM type for storing user information. Since redis is used for this ephemerally
//! in most cases this is very simple

use botapi::gen_types::{InlineKeyboardButton, InlineKeyboardButtonBuilder};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DeriveEntityModel, Eq, Hash)]
#[sea_orm(table_name = "button")]
pub struct Model {
    pub button_text: String,
    pub callback_data: Option<String>,
    pub button_url: Option<String>,
    pub owner_id: Option<i64>,
    #[sea_orm(primary_key)]
    pub pos_x: i32,
    #[sea_orm(primary_key)]
    pub pos_y: i32,
    pub raw_text: Option<String>,
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

    pub fn to_button_owned(&self) -> InlineKeyboardButton {
        let mut b = InlineKeyboardButtonBuilder::new(self.button_text.to_owned());

        if let Some(ref text) = self.callback_data {
            b = b.set_callback_data(text.to_owned());
        }

        if let Some(ref url) = self.button_url {
            b = b.set_url(url.to_owned());
        }
        b.build()
    }

    pub fn from_button(
        pos_x: i32,
        pos_y: i32,
        button: &InlineKeyboardButton,
        owner_id: i64,
        raw_text: Option<String>,
    ) -> Model {
        Model {
            pos_x,
            pos_y,
            button_text: button.get_text().to_owned(),
            button_url: button.get_url().map(|v| v.to_owned()),
            owner_id: Some(owner_id),
            callback_data: button.get_callback_data().map(|v| v.to_owned()),
            raw_text,
        }
    }

    pub fn from_button_orphan(
        pos_x: i32,
        pos_y: i32,
        button: &InlineKeyboardButton,
        raw_text: Option<String>,
    ) -> Model {
        Model {
            pos_x,
            pos_y,
            button_text: button.get_text().to_owned(),
            button_url: button.get_url().map(|v| v.to_owned()),
            owner_id: None,
            callback_data: button.get_callback_data().map(|v| v.to_owned()),
            raw_text,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
