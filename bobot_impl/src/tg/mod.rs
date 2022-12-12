use crate::util::error::BotError;

pub type Result<T> = anyhow::Result<T, BotError>;

pub mod client;
#[allow(dead_code)]
pub(crate) mod dialog;

pub(crate) mod command;
pub(crate) mod user;
