//! This module defines button related APIs for creating inline keyboards on messages,
//! handling callbacks for clicked buttons, and handling deep links

use crate::persist::core::button;
use crate::statics::ME;
use crate::util::error::Result;
use crate::{statics::TG, util::error::BotError};
use botapi::gen_types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
};
use futures::Future;
use serde::{Deserialize, Serialize};

const MAX_BUTTONS: usize = 8;

/// Builds an inline keyboard with buttons for attaching to a message
#[derive(Clone, Serialize, Deserialize)]
pub struct InlineKeyboardBuilder(Vec<Vec<button::Model>>);

impl Default for InlineKeyboardBuilder {
    fn default() -> Self {
        Self(vec![vec![]])
    }
}

/// Formats a string into a deep linking url for this bot
pub fn get_url<T: AsRef<str>>(param: T) -> Result<String> {
    let me = ME.get().unwrap();
    let url = format!(
        "https://t.me/{}?start={}",
        me.get_username_ref()
            .ok_or_else(|| BotError::Generic("help I don't have a username".to_owned()))?,
        param.as_ref()
    );
    Ok(url)
}

impl InlineKeyboardBuilder {
    pub fn from_vec(value: Vec<Vec<button::Model>>) -> Self {
        Self(value)
    }

    pub fn get_mut<'a>(&'a mut self) -> &'a mut Vec<Vec<button::Model>> {
        &mut self.0
    }

    /// Adds a new button to the inline keyboard row, autowrapping if needed
    pub fn button_raw(
        &mut self,
        button: InlineKeyboardButton,
        raw_text: Option<String>,
    ) -> &mut Self {
        let len = self.0.len() as i32;
        if let Some(v) = self.0.last_mut() {
            if v.len() < MAX_BUTTONS {
                v.push(button::Model::from_button_orphan(
                    v.len() as i32,
                    len,
                    &button,
                    raw_text,
                ));
                self
            } else {
                self.newline().button(button)
            }
        } else {
            self
        }
    }

    pub fn button(&mut self, button: InlineKeyboardButton) -> &mut Self {
        self.button_raw(button, None)
    }

    /// get the length of the current button row
    pub fn row_len(&self) -> usize {
        self.0.last().map(|v| v.len()).unwrap_or(0)
    }

    /// Adds a button that sends a command to the current chat
    pub fn command_button(&mut self, caption: String, command: String) -> &mut Self {
        let b = InlineKeyboardButtonBuilder::new(caption)
            .set_switch_inline_query_current_chat(command)
            .build();
        self.button(b)
    }

    /// Moves the current line to the next line without adding a new button
    pub fn newline(&mut self) -> &mut Self {
        self.0.push(vec![]);
        self
    }

    /// gets mutable access to stored vec
    pub fn get<'a>(&'a self) -> &'a Vec<Vec<button::Model>> {
        &self.0
    }

    pub fn into_inner(self) -> Vec<Vec<button::Model>> {
        self.0
    }

    /// Generates an InlineKeyboardMarkup for use in telegram api types
    pub fn build(self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(
            self.0
                .into_iter()
                .map(|v| v.into_iter().map(|v| v.to_button()).collect())
                .collect(),
        )
    }

    pub fn build_owned(&mut self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(
            self.0
                .drain(..)
                .map(|mut v| v.drain(..).map(|v| v.to_button()).collect())
                .collect(),
        )
    }
}

/// Extension trait for registing callback on buttons.
/// Beware, this calls functions in static contexts
pub trait OnPush {
    /// Register a button callback that is only called once, then unregistered
    fn on_push<'a, F, Fut>(&self, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static;

    /// Register a button callback that is called until it returns false
    fn on_push_multi<'a, F, Fut>(&self, func: F)
    where
        F: Fn(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static;
}

impl OnPush for InlineKeyboardButton {
    fn on_push<'a, F, Fut>(&self, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        TG.register_button(self, func);
    }

    fn on_push_multi<'a, F, Fut>(&self, func: F)
    where
        F: Fn(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        TG.register_button_multi(self, func);
    }
}
