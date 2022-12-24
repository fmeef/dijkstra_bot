use nonblock_logger::{
    log::LevelFilter, BaseConsumer, BaseFilter, BaseFormater, JoinHandle, NonblockLogger,
};

use serde::{Deserialize, Serialize};

use std::io;

pub(crate) struct LevelFilterWrapper(pub(crate) LevelFilter);

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

pub fn setup_log() -> JoinHandle {
    let formater = BaseFormater::new().local(true).color(true).level(4);

    let filter = BaseFilter::new()
        .starts_with(true)
        .max_level(LevelFilter::Info);
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
