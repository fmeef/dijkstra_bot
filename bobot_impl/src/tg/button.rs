use anyhow::Result;
use botapi::gen_types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup, Message,
};
use futures::Future;

use crate::{persist::redis::scope_key_by_chatuser, statics::TG};

const MAX_BUTTONS: usize = 8;

pub(crate) struct InlineKeyboardBuilder(Vec<Vec<InlineKeyboardButton>>);

impl Default for InlineKeyboardBuilder {
    fn default() -> Self {
        Self(vec![vec![]])
    }
}

impl InlineKeyboardBuilder {
    pub(crate) fn button(mut self, button: InlineKeyboardButton) -> Self {
        if let Some(v) = self.0.last_mut() {
            if v.len() < MAX_BUTTONS {
                v.push(button);
                self
            } else {
                self.newline().button(button)
            }
        } else {
            self
        }
    }

    pub(crate) fn command_button(self, caption: String, command: String) -> Self {
        let b = InlineKeyboardButtonBuilder::new(caption)
            .set_switch_inline_query_current_chat(command)
            .build();
        self.button(b)
    }

    pub(crate) fn newline(mut self) -> Self {
        self.0.push(vec![]);
        self
    }

    pub(crate) fn build(self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(self.0)
    }
}

pub(crate) trait OnPush {
    fn on_push<'a, F, Fut>(&self, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static;
}

impl OnPush for InlineKeyboardButton {
    fn on_push<'a, F, Fut>(&self, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        TG.register_button(self, func);
    }
}
