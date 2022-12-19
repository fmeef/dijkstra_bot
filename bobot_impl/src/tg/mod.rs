use crate::util::error::BotError;

pub type Result<T> = anyhow::Result<T, BotError>;

pub(crate) mod button;
pub mod client;
pub(crate) mod command;
#[allow(dead_code)]
pub(crate) mod dialog;
pub(crate) mod user;
