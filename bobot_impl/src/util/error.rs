use botapi::bot::ApiError;
use sea_orm::{DbErr, TransactionError};
use thiserror::Error;
use tokio::task::JoinError;

use crate::{statics::TG, tg::markdown::DefaultParseErr};

pub type Result<T> = std::result::Result<T, BotError>;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("{say}")]
    Speak {
        say: String,
        chat: i64,
        err: Option<Box<BotError>>,
    },
    #[error("Telegram API error")]
    ApiError(#[from] ApiError),
    #[error("Invalid conversation: {0}")]
    ConversationError(String),
    #[error("internal redis error: {0}")]
    RedisErr(#[from] redis::RedisError),
    #[error("redis pool error: {0}")]
    RedisPoolErr(#[from] bb8::RunError<redis::RedisError>),
    #[error("serialization error: {0}")]
    SerializationErr(#[from] rmp_serde::encode::Error),
    #[error("deserialization error {0}")]
    DeserializationErr(#[from] rmp_serde::decode::Error),
    #[error("nursery error {0}")]
    NurseryErr(#[from] async_nursery::NurseErr),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("DB error: {0}")]
    DbError(#[from] sea_orm::DbErr),
    #[error("DB runtime error: {0}")]
    DbRuntimeError(#[from] sea_orm::RuntimeErr),
    #[error("Murkdown parse error")]
    MurkdownError(#[from] DefaultParseErr),
    #[error("Tokio join error")]
    JoinErr(#[from] JoinError),
    #[error("Uuid error: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("Hyper error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("Transaction error {0}")]
    TransactionErr(#[from] TransactionError<DbErr>),
    #[error("Generic error {0}")]
    Generic(String),
}

impl BotError {
    pub fn conversation_err<T: Into<String>>(text: T) -> Self {
        Self::ConversationError(text.into())
    }
    pub fn speak<T: Into<String>>(text: T, chat: i64) -> Self {
        Self::Speak {
            say: text.into(),
            chat,
            err: None,
        }
    }

    pub fn speak_err<T, E>(text: T, chat: i64, err: E) -> Self
    where
        T: Into<String>,
        E: Into<BotError>,
    {
        Self::Speak {
            say: text.into(),
            chat,
            err: Some(Box::new(err.into())),
        }
    }

    pub fn record_stats(&self) {
        if let Self::ApiError(ref error) = self {
            if let Some(error) = error.get_response() {
                if let Some(error_code) = error.error_code {
                    crate::statics::count_error_code(error_code);
                }
            }
        }
    }

    pub async fn get_message(&self) -> Result<()> {
        if let Self::Speak { say, chat, .. } = self {
            TG.client().build_send_message(*chat, &say).build().await?;
        }

        Ok(())
    }
}
