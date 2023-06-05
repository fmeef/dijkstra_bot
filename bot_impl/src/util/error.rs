//! Unified error handling for everything in this project.
//! Provides an error type using thiserror to handle and route errors from other
//! components.
//!
//! Also provides helper functions for either logging errors to prometheus or
//! sending formatted errors to the user via telegram

use crate::{statics::TG, tg::markdown::DefaultParseErr};
use async_trait::async_trait;
use botapi::bot::{ApiError, Response};
use botapi::gen_types::Chat;
use chrono::OutOfRangeError;
use sea_orm::{DbErr, TransactionError};
use thiserror::Error;
use tokio::task::JoinError;

/// Type alias for universal result type
pub type Result<T> = std::result::Result<T, BotError>;

/// Extension trait for mapping generic errors into BotError::Speak
/// Meant to be implemented on Result
#[async_trait]
pub trait SpeakErr<T: Send> {
    /// Maps the error to BotError::Speak using a custom function to derive error message
    async fn speak_err_fmt<F>(self, chat: &Chat, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b str) -> String + Send;

    /// Maps the error to BotError::Speak using a custom function to derive error message
    async fn speak_err_raw<F>(self, chat: &Chat, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b BotError) -> Option<String> + Send;

    async fn speak_err<F>(self, chat: &Chat, code: i64, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b Response) -> String + Send;
}

#[async_trait]
impl<T: Send> SpeakErr<T> for Result<T> {
    async fn speak_err_fmt<F>(self, chat: &Chat, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b str) -> String + Send,
    {
        match self {
            Err(err) => match err {
                BotError::ApiError(_) => {
                    let message = err.get_tg_error();
                    let err = func(message);
                    Err(BotError::speak(err, chat.get_id()))
                }
                BotError::Speak { .. } => Err(err),
                err => {
                    let message = err.to_string();
                    let err = func(&message);
                    Err(BotError::speak(err, chat.get_id()))
                }
            },
            Ok(v) => Ok(v),
        }
    }

    /// Maps the error to BotError::Speak using a custom function to derive error message
    async fn speak_err_raw<F>(self, chat: &Chat, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b BotError) -> Option<String> + Send,
    {
        if let Err(ref err) = self {
            if let Some(message) = func(err) {
                Err(BotError::speak(message, chat.get_id()))
            } else {
                self
            }
        } else {
            self
        }
    }

    async fn speak_err<F>(self, chat: &Chat, code: i64, func: F) -> Result<T>
    where
        F: for<'b> FnOnce(&'b Response) -> String + Send,
    {
        if let Err(BotError::ApiError(ref err)) = self {
            if let Some(resp) = err.get_response() {
                if !resp.ok && resp.error_code == Some(code) {
                    let message = func(resp);
                    return Err(BotError::speak(message, chat.get_id()));
                }
            }
        }
        self
    }
}

/// thiserror enum for all possible errors
#[derive(Debug, Error)]
pub enum BotError {
    #[error("{say}")]
    Speak {
        say: String,
        chat: i64,
        err: Option<Box<BotError>>,
    },
    #[error("Telegram API error: {0}")]
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
    #[error("Time out of range {0}")]
    TimeOutOfRange(#[from] OutOfRangeError),
    #[error("Base64 decode error {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Invalid glob pattern: {0}")]
    GlobError(#[from] globset::Error),
    #[error("Generic error {0}")]
    Generic(String),
}

impl BotError {
    /// constructor for conversation state machine error
    pub fn conversation_err<T: Into<String>>(text: T) -> Self {
        Self::ConversationError(text.into())
    }

    /// constructor for "speak" error that is always converted into telegram message
    pub fn speak<T: Into<String>>(text: T, chat: i64) -> Self {
        Self::Speak {
            say: text.into(),
            chat,
            err: None,
        }
    }

    /// construct a speak error with custom error type
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

    /// record this error using prometheus error counters. Counters used depend on error
    pub fn record_stats(&self) {
        if let Self::ApiError(ref error) = self {
            if let Some(error) = error.get_response() {
                log::error!(
                    "telegram error code {} {}",
                    error
                        .error_code
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "invalid".to_owned()),
                    error
                        .description
                        .as_ref()
                        .map(|v| v.as_str())
                        .unwrap_or("no description")
                );
                if let Some(error_code) = error.error_code {
                    crate::persist::metrics::count_error_code(error_code);
                }
            }
        }
    }

    /// get humanreadable error string to print to user via telegram
    pub fn get_tg_error<'a>(&'a self) -> &'a str {
        if let BotError::ApiError(err) = self {
            err.get_response()
                .map(|r| r.description.as_ref().map(|v| v.as_str()))
                .flatten()
                .unwrap_or("")
        } else {
            ""
        }
    }

    /// send message via telegram for this error, returning true if a message was sent
    pub async fn get_message(&self) -> Result<bool> {
        if let Self::Speak { say, chat, .. } = self {
            TG.client().build_send_message(*chat, &say).build().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
