//! Logging setup and configuration
//!
//! currently using nonblock_logger, we need to implement Serialize and Deserialize for log
//! types to allow configuring logs via the configuration file

use nonblock_logger::log::LevelFilter;

use serde::{Deserialize, Serialize};

#[cfg(not(test))]
use nonblock_logger::{BaseConsumer, BaseFilter, BaseFormater, JoinHandle, NonblockLogger};

#[cfg(not(test))]
use std::io;

#[cfg(not(test))]
use crate::statics::CONFIG;

#[derive(Debug)]
pub struct LevelFilterWrapper(pub LevelFilter);

impl Serialize for LevelFilterWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0 {
            LevelFilter::Off => serializer.serialize_str("off"),
            LevelFilter::Error => serializer.serialize_str("error"),
            LevelFilter::Warn => serializer.serialize_str("warn"),
            LevelFilter::Info => serializer.serialize_str("info"),
            LevelFilter::Debug => serializer.serialize_str("debug"),
            LevelFilter::Trace => serializer.serialize_str("trace"),
        }
    }
}

impl<'de> Deserialize<'de> for LevelFilterWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let r = String::deserialize(deserializer)?;

        match r.as_str() {
            "off" => Ok(Self(LevelFilter::Off)),
            "error" => Ok(Self(LevelFilter::Error)),
            "warn" => Ok(Self(LevelFilter::Warn)),
            "info" => Ok(Self(LevelFilter::Info)),
            "debug" => Ok(Self(LevelFilter::Debug)),
            "trace" => Ok(Self(LevelFilter::Trace)),
            _ => Err(serde::de::Error::unknown_variant(
                r.as_str(),
                &["off", "error", "info", "warn", "debug", "trace"],
            )),
        }
    }
}

/// Setup logging and start logger thread
#[cfg(not(test))]
pub(crate) fn setup_log() -> JoinHandle {
    let formater = BaseFormater::new().local(true).color(true).level(4);

    let filter = BaseFilter::new()
        .starts_with(true)
        .max_level(CONFIG.logging.get_log_level());
    let consumer = BaseConsumer::stdout(filter.max_level_get())
        .chain(LevelFilter::Error, io::stderr())
        .unwrap();

    let logger = NonblockLogger::new()
        .formater(formater)
        .filter(filter)
        .and_then(|l| l.consumer(consumer))
        .unwrap();
    logger
        .spawn()
        .map_err(|e| eprintln!("failed to init nonblock_logger: {:?}", e))
        .unwrap()
}
