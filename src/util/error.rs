use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("Generic internal error, am cry: {0}")]
    InternalError(String),
    #[error("{0}")]
    AlreadyExistsError(String),
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
}

impl BotError {
    pub fn new<T: ToString>(reason: T) -> Self {
        BotError::InternalError(reason.to_string())
    }

    pub fn from(err: Box<dyn std::error::Error>) -> Self {
        BotError::InternalError(err.to_string())
    }
}
