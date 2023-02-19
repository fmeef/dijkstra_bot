use crate::logger::LevelFilterWrapper;
use crate::persist::redis::{RedisPool, RedisPoolBuilder};
use crate::tg::client::TgClient;

use chrono::Duration;
use clap::Parser;
use confy::load_path;
use dashmap::DashMap;
use futures::executor::block_on;
use lazy_static::lazy_static;
use log::LevelFilter;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};
use sea_orm::entity::prelude::DatabaseConnection;
use sea_orm::{ConnectOptions, Database};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::runtime::Runtime;

#[derive(Serialize, Deserialize)]
pub struct WebhookConfig {
    pub enable_webhook: bool,
    pub webhook_url: String,
    pub listen: SocketAddr,
}

#[derive(Serialize, Deserialize)]
pub struct LogConfig {
    log_level: LevelFilterWrapper,
    pub prometheus_hook: SocketAddr,
}

#[derive(Serialize, Deserialize)]
pub struct Persistence {
    pub database_connection: String,
    pub redis_connection: String,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub bot_token: String,
    pub persistence: Persistence,
    pub webhook: WebhookConfig,
    pub logging: LogConfig,
    pub timing: Timing,
}

#[derive(Serialize, Deserialize)]
pub struct Timing {
    pub cache_timeout: usize,
    pub antifloodwait_count: usize,
    pub antifloodwait_time: usize,
    pub ignore_chat_time: usize,
}

impl LogConfig {
    pub fn get_log_level(&self) -> LevelFilter {
        self.log_level.0
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            cache_timeout: Duration::hours(48).num_seconds() as usize,
            antifloodwait_count: 80,
            antifloodwait_time: 150,
            ignore_chat_time: Duration::minutes(10).num_seconds() as usize,
        }
    }
}

impl Default for Persistence {
    fn default() -> Self {
        Self {
            redis_connection: "redis://localhost".to_owned(),
            database_connection: "postgresql://user:password@localhost/database".to_owned(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            log_level: LevelFilterWrapper(log::LevelFilter::Info),
            prometheus_hook: ([0, 0, 0, 0], 9999).into(),
        }
    }
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enable_webhook: false,
            webhook_url: "https://bot.ustc.edu.cn".to_owned(),
            listen: ([0, 0, 0, 0], 8080).into(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bot_token: "changeme".to_owned(),
            persistence: Persistence::default(),
            logging: LogConfig::default(),
            webhook: WebhookConfig::default(),
            timing: Timing::default(),
        }
    }
}

// Mildly competent moduler telegram bot
#[derive(Parser)]
#[clap(author, version, long_about = None)]
pub struct Args {
    // Path to config file
    #[clap(short, long)]
    pub config: PathBuf,
}

lazy_static! {
    pub static ref EXEC: Runtime = {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("create tokio threadpool")
    };
}

//global configuration parameters
lazy_static! {
    pub static ref ARGS: Args = Args::parse();
    pub static ref CONFIG: Config = load_path(&ARGS.config).expect("failed to load config");
}

//redis client
lazy_static! {
    pub static ref REDIS: RedisPool =
        block_on(RedisPoolBuilder::new(&CONFIG.persistence.redis_connection).build())
            .expect("failed to initialize redis pool");
}

//db client
lazy_static! {
    pub static ref DB: DatabaseConnection = Runtime::new().unwrap().block_on(async move {
        let db = Database::connect(ConnectOptions::new(
            CONFIG.persistence.database_connection.to_owned(),
        ))
        .await
        .expect("failed to initialize database");
        db
    });
}

//tg client
lazy_static! {
    pub static ref TG: TgClient = TgClient::connect(CONFIG.bot_token.to_owned());
}

//counters
lazy_static! {
    pub static ref TEST_COUNTER: IntCounter =
        register_int_counter!("testlabel", "testhelp").unwrap();
    pub static ref ERROR_CODES: Histogram =
        register_histogram!("module_fails", "Telegram api cries").unwrap();
    pub static ref ERROR_CODES_MAP: DashMap<i64, IntCounter> = DashMap::new();
}

pub fn count_error_code(err: i64) {
    let counter = ERROR_CODES_MAP.entry(err).or_insert_with(|| {
        register_int_counter!(format! {"errcode_{}", err}, "Telegram error counter").unwrap()
    });
    counter.value().inc();
}
