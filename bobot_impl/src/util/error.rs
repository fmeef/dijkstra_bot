use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("{say}")]
    Speak {
        say: String,
        err: Option<Box<BotError>>,
    },
    #[error("Generic internal error, am cry: {0}")]
    InternalError(String),
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
}

impl BotError {
    pub fn new<T: ToString>(reason: T) -> Self {
        BotError::InternalError(reason.to_string())
    }
}
