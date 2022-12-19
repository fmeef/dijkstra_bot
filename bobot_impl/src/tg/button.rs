use botapi::gen_types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup};
use futures::Future;

use crate::statics::TG;

pub(crate) struct InlineKeyboardBuilder(Vec<Vec<InlineKeyboardButton>>);

impl Default for InlineKeyboardBuilder {
    fn default() -> Self {
        Self(vec![vec![]])
    }
}

impl InlineKeyboardBuilder {
    pub(crate) fn button(mut self, button: InlineKeyboardButton) -> Self {
        if let Some(v) = self.0.last_mut() {
            v.push(button)
        }
        self
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
