use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("Generic internal error, am cry: {0}")]
    InternalError(String),
    #[error("internal redis error: {0}")]
    RedisErr(#[from] redis::RedisError),
    #[error("redis pool error")]
    RedisPoolErr(#[from] bb8::RunError<redis::RedisError>),
    #[error("serialization error")]
    SerializationErr(#[from] rmp_serde::encode::Error),
    #[error("deserialization error")]
    DeserializationErr(#[from] rmp_serde::decode::Error),
    #[error("nursery error")]
    NurseryErr(#[from] async_nursery::NurseErr),
    #[error("telegram authorization error")]
    TgAuthErr(#[from] grammers_client::client::auth::AuthorizationError),
    #[error("telegram invocation error")]
    TgInvocationError(#[from] grammers_client::client::auth::InvocationError),
    #[error("io error")]
    IoError(#[from] std::io::Error),
}

impl BotError {
    pub fn new<T: ToString>(reason: T) -> Self {
        BotError::InternalError(reason.to_string())
    }
}
