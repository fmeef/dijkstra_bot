use botapi::gen_types::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
};
use futures::Future;

use super::user::get_me;
use crate::util::error::Result;
use crate::{statics::TG, util::error::BotError};

const MAX_BUTTONS: usize = 8;

pub struct InlineKeyboardBuilder(Vec<Vec<InlineKeyboardButton>>);

impl Default for InlineKeyboardBuilder {
    fn default() -> Self {
        Self(vec![vec![]])
    }
}

pub async fn get_url<T: AsRef<str>>(param: T) -> Result<String> {
    let me = get_me().await?;
    let url = format!(
        "https://t.me/{}?start={}",
        me.get_username_ref()
            .ok_or_else(|| BotError::Generic("help I don't have a username".to_owned()))?,
        param.as_ref()
    );
    Ok(url)
}

#[allow(dead_code)]
impl InlineKeyboardBuilder {
    pub fn button(mut self, button: InlineKeyboardButton) -> Self {
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

    pub fn row_len(&self) -> usize {
        self.0.last().map(|v| v.len()).unwrap_or(0)
    }

    pub fn command_button(self, caption: String, command: String) -> Self {
        let b = InlineKeyboardButtonBuilder::new(caption)
            .set_switch_inline_query_current_chat(command)
            .build();
        self.button(b)
    }

    pub fn newline(mut self) -> Self {
        self.0.push(vec![]);
        self
    }

    pub fn build(self) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(self.0)
    }
}

pub trait OnPush {
    fn on_push<'a, F, Fut>(&self, func: F)
    where
        F: FnOnce(CallbackQuery) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static;

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
