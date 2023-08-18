//! Due to limitations of the borrow checker when dealing with static async contexts,
//! passing non-'static references to tokio tasks is very hard.
//!
//! Make critical parts of the bot's backend static to avoid loads of boilerplate
//! or Arc::clone() calls

use crate::logger::LevelFilterWrapper;
use crate::persist::redis::{RedisPool, RedisPoolBuilder};
use crate::tg::client::TgClient;

use botapi::gen_types::User;
use chrono::Duration;
use clap::Parser;
use confy::load_path;
use futures::executor::block_on;
use governor::clock::QuantaClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use lazy_static::lazy_static;
use log::LevelFilter;
use sea_orm::entity::prelude::DatabaseConnection;
use sea_orm::{ConnectOptions, Database};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use tokio::sync::OnceCell;

use tokio::runtime::Runtime;

/// Serializable log config for webhook
#[derive(Serialize, Deserialize)]
pub struct WebhookConfig {
    /// if true, use webhook, if false, use long polling
    pub enable_webhook: bool,

    /// webhook url if using webhook
    pub webhook_url: String,

    /// if using webhook listen on this socket
    pub listen: SocketAddr,
}

/// Administration and moderation options
#[derive(Serialize, Deserialize)]
pub struct Admin {
    /// Users with special administrative access on the bot
    pub sudo_users: HashSet<i64>,
    pub support_users: HashSet<i64>,
}

/// Serializable log setup config
#[derive(Serialize, Deserialize)]
pub struct LogConfig {
    /// log level, one of "off", "error", "warn", "info", "debug", "trace"
    log_level: LevelFilterWrapper,

    /// socket to listen on for prometheus scraping
    pub prometheus_hook: SocketAddr,
}

/// Serializable config for postgres and redis
#[derive(Serialize, Deserialize)]
pub struct Persistence {
    /// postgres connection string
    pub database_connection: String,

    /// redis connection string
    pub redis_connection: String,
}

/// Main configuration file contents. Serializable to toml
#[derive(Serialize, Deserialize)]
pub struct Config {
    /// telegram bot api token
    pub bot_token: String,
    pub persistence: Persistence,
    pub webhook: WebhookConfig,
    pub logging: LogConfig,
    pub timing: Timing,
    pub admin: Admin,
}

/// Serializable timing config
#[derive(Serialize, Deserialize)]
pub struct Timing {
    /// default redis key expiry
    pub cache_timeout: usize,

    /// number of messages to trigger antiflood
    pub antifloodwait_count: usize,

    /// time before antiflood counter resets
    pub antifloodwait_time: usize,

    /// how long to ignore chat when triggering antiflood
    pub ignore_chat_time: usize,
}

impl LogConfig {
    pub fn get_log_level(&self) -> LevelFilter {
        self.log_level.0
    }
}

impl Default for Admin {
    fn default() -> Self {
        Self {
            sudo_users: HashSet::new(),
            support_users: HashSet::new(),
        }
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
            admin: Admin::default(),
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
    pub static ref ME: OnceCell<User> = OnceCell::new();
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
    pub static ref DB: DatabaseConnection = EXEC.block_on(async move {
        let db = Database::connect(ConnectOptions::new(
            CONFIG.persistence.database_connection.to_owned(),
        ))
        .await
        .expect("failed to initialize database");
        db
    });
}

lazy_static! {
    pub static ref BAN_GOVERNER: RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware> =
        RateLimiter::direct(Quota::per_second(NonZeroU32::new(30u32).unwrap()));
}

//tg client
lazy_static! {
    pub static ref TG: TgClient = TgClient::connect(CONFIG.bot_token.to_owned());
}
