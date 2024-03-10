//! Unified error handling for everything in this project.
//! Provides an error type using thiserror to handle and route errors from other
//! components.
//!
//! Also provides helper functions for either logging errors to prometheus or
//! sending formatted errors to the user via telegram
use std::time::SystemTimeError;

use crate::tg::command::Context;
use crate::tg::markdown::DefaultParseErr;
use async_trait::async_trait;
use bb8::RunError;
use botapi::bot::{ApiError, Response};
use botapi::gen_types::{Chat, Message};
use chrono::OutOfRangeError;
use redis::RedisError;
use sea_orm::{DbErr, RuntimeErr, TransactionError};
use sqlx::error::DatabaseError;
use thiserror::Error;
use tokio::task::JoinError;

use super::string::Speak;

/// Type alias for universal result type
pub type Result<T> = std::result::Result<T, BotError>;

/// Extension trait for mapping generic errors into BotError::Speak
/// Meant to be implemented on Result
#[async_trait]
pub trait SpeakErr<T: Send> {
    /// Maps the error to BotError::Speak using a custom function to derive error message
    async fn speak_err<F, U>(self, ctx: &U, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b str) -> String + Send;

    /// Maps the error to BotError::Speak using a custom function to derive error message
    /// returning None for the error message causes the error to be passed verbatim
    async fn speak_err_raw<F, U>(self, ctx: &U, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b BotError) -> Option<String> + Send;

    /// Maps the error to BotError::Speak using a custom function only if the telegram error code
    /// matches
    async fn speak_err_code<F, U>(self, ctx: &U, code: i64, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b Response) -> String + Send;

    /// Maps the database error code to BotError::Speak using a custom function only if the
    /// db error code matches
    async fn speak_db_code<F, U>(self, ctx: &U, code: &str, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b dyn DatabaseError) -> String + Send;

    async fn silent(self) -> Result<T>;

    fn log(self) -> Option<T>;
}

#[async_trait]
impl<T: Send, E: Into<BotError> + Send> SpeakErr<T> for std::result::Result<T, E> {
    fn log(self) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(err) => {
                let err = err.into();
                log::warn!("error {}", err);
                err.record_stats();
                None
            }
        }
    }

    async fn speak_err<F, U>(self, ctx: &U, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b str) -> String + Send,
    {
        match self {
            Err(err) => {
                let err = err.into();
                match err {
                    BotError::ApiError(_) => {
                        let message = err.get_tg_error();
                        let err = func(message);
                        ctx.fail(err)
                    }
                    err => {
                        let message = err.to_string();
                        let err = func(&message);
                        ctx.fail(err)
                    }
                }
            }
            Ok(v) => Ok(v),
        }
    }

    /// Maps the error to BotError::Speak using a custom function to derive error message
    async fn speak_err_raw<F, U>(self, ctx: &U, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b BotError) -> Option<String> + Send,
    {
        let self = self.map_err(|e| e.into());
        if let Err(ref err) = self {
            if let Some(message) = func(err) {
                ctx.fail(message)
            } else {
                self
            }
        } else {
            self
        }
    }

    async fn speak_err_code<F, U>(self, ctx: &U, code: i64, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b Response) -> String + Send,
    {
        let self = self.map_err(|e| e.into());
        if let Err(BotError::ApiError(ref err)) = self {
            if let Some(resp) = err.get_response() {
                if !resp.ok && resp.error_code == Some(code) {
                    let message = func(resp);
                    return ctx.fail(message);
                }
            }
        }
        self
    }

    async fn speak_db_code<F, U>(self, ctx: &U, code: &str, func: F) -> Result<T>
    where
        U: Fail + Send + Sync,
        F: for<'b> FnOnce(&'b dyn DatabaseError) -> String + Send,
    {
        let self = self.map_err(|e| e.into());
        if let Err(BotError::DbError(DbErr::Exec(RuntimeErr::SqlxError(ref err)))) = self {
            if let Some(err) = err.as_database_error() {
                log::warn!("db error: {:?}", err);
                if err.code().map(|v| v.as_ref() == code).unwrap_or(false) {
                    let message = func(err);
                    return ctx.fail(message);
                }
            }
        }
        self
    }

    async fn silent(self) -> Result<T> {
        match self.map_err(|e| e.into()) {
            Err(BotError::Speak { err: Some(err), .. }) => Err(*err),
            Err(BotError::Speak { say, err: None, .. }) => Err(BotError::Generic(say)),
            v => v,
        }
    }
}

/// Helper trait for constructing a BotError::Speak
pub trait Fail {
    /// construct a result that always returns Err(BotError::Speak)
    fn fail<T: AsRef<str>, R>(&self, message: T) -> Result<R>;
    /// construct a BotError::Speak
    fn fail_err<T: AsRef<str>>(&self, message: T) -> BotError;
}

impl Fail for Context {
    fn fail<T: AsRef<str>, R>(&self, message: T) -> Result<R> {
        Err(self.fail_err(message))
    }

    fn fail_err<T: AsRef<str>>(&self, message: T) -> BotError {
        match self.try_get() {
            Ok(get) => BotError::speak(message.as_ref(), get.chat.get_id()),
            Err(err) => err,
        }
    }
}

impl Fail for Message {
    fn fail<T: AsRef<str>, R>(&self, message: T) -> Result<R> {
        Err(self.fail_err(message))
    }

    fn fail_err<T: AsRef<str>>(&self, message: T) -> BotError {
        BotError::speak(message.as_ref(), self.get_chat().get_id())
    }
}

impl Fail for Chat {
    fn fail<T: AsRef<str>, R>(&self, message: T) -> Result<R> {
        Err(self.fail_err(message))
    }

    fn fail_err<T: AsRef<str>>(&self, message: T) -> BotError {
        BotError::speak(message.as_ref(), self.get_id())
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
    RedisPoolErr(#[from] RunError<RedisError>),
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
    #[error("Json serialization error: {0}")]
    SerdeJsonErr(#[from] serde_json::Error),
    #[error("Http error {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("{0}")]
    Generic(String),
    #[error("User not found")]
    UserNotFound,
    #[error("Query error {0}")]
    QueryError(#[from] sea_query::error::Error),
    #[error("System time error: {0}")]
    SystemTimeError(#[from] SystemTimeError),
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for BotError {
    fn from(value: tokio::sync::mpsc::error::SendError<T>) -> Self {
        BotError::Generic(value.to_string())
    }
}

// impl<T> From<RunError<T>> for BotError {
//     fn from(value: RunError<T>) -> Self {
//         Self::RedisPoolErr("Redis pool error".to_owned())
//     }
// }

impl From<TransactionError<BotError>> for BotError {
    fn from(value: TransactionError<BotError>) -> Self {
        BotError::Generic(value.to_string())
    }
}

impl BotError {
    /// constructor for conversation state machine error
    pub fn conversation_err<T: Into<String>>(text: T) -> Self {
        Self::ConversationError(text.into())
    }

    /// Generic and silent error
    pub fn generic<T: ToString>(text: T) -> Self {
        Self::Generic(text.to_string())
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
                log::warn!(
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
                if let Some(ref extra) = error.floods {
                    for error in extra {
                        if let Some(error_code) = error.error_code {
                            crate::persist::metrics::count_error_code(error_code);
                        }
                    }
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
            chat.speak(say).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
