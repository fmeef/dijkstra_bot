use crate::util::error::BotError;

pub type Result<T> = anyhow::Result<T, BotError>;

pub mod client;
