//! Due to limitations of the borrow checker when dealing with static async contexts,
//! passing non-'static references to tokio tasks is very hard.
//!
//! Make critical parts of the bot's backend static to avoid loads of boilerplate
//! or Arc::clone() calls

use crate::logger::LevelFilterWrapper;
#[cfg(test)]
use crate::persist::redis::MockPool;
use crate::persist::redis::RedisPool;
use crate::tg::client::TgClient;
#[cfg(not(test))]
use bb8_redis::RedisConnectionManager;
use botapi::gen_types::User;
use chrono::Duration;
use clap::Parser;
use governor::clock::QuantaClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use lazy_static::lazy_static;
use log::LevelFilter;
use once_cell::sync::OnceCell;
#[cfg(not(test))]
use redis::aio::MultiplexedConnection;
use sea_orm::entity::prelude::DatabaseConnection;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use tokio::runtime::Runtime;

/// Serializable log config for webhook
#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookConfig {
    /// if true, use webhook, if false, use long polling
    pub enable_webhook: bool,

    /// webhook url if using webhook
    pub webhook_url: String,

    /// if using webhook listen on this socket
    pub listen: SocketAddr,
}

/// Administration and moderation options
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Admin {
    /// Users with special administrative access on the bot
    pub sudo_users: HashSet<i64>,
    pub support_users: HashSet<i64>,
}

/// Serializable log setup config
#[derive(Serialize, Deserialize, Debug)]
pub struct LogConfig {
    /// log level, one of "off", "error", "warn", "info", "debug", "trace"
    log_level: LevelFilterWrapper,

    /// socket to listen on for prometheus scraping
    pub prometheus_hook: SocketAddr,
}

/// Serializable config for postgres and redis
#[derive(Serialize, Deserialize, Debug)]
pub struct Persistence {
    /// postgres connection string
    pub database_connection: String,

    /// redis connection string
    pub redis_connection: String,
}

/// Main configuration file contents. Serializable to toml
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    /// telegram bot api token
    pub bot_token: String,
    pub modules: Modules,
    pub persistence: Persistence,
    pub webhook: WebhookConfig,
    pub logging: LogConfig,
    pub timing: Timing,
    pub admin: Admin,
    pub compute_threads: usize,
}

/// Configuration for loadable modules
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Modules {
    /// List of modules to disable
    pub disabled: HashSet<String>,

    /// Allowlist of modules to enable, overrides the disabled option
    pub enabled: HashSet<String>,
}

/// Serializable timing config
#[derive(Serialize, Deserialize, Debug)]
pub struct Timing {
    /// default redis key expiry
    pub cache_timeout: i64,

    /// number of messages to trigger antiflood
    pub antifloodwait_count: usize,

    /// time before antiflood counter resets
    pub antifloodwait_time: i64,

    /// how long to ignore chat when triggering antiflood
    pub ignore_chat_time: i64,
}

pub fn module_enabled(module: &str) -> bool {
    if CONFIG.modules.enabled.is_empty() {
        !CONFIG.modules.disabled.contains(module)
    } else {
        CONFIG.modules.enabled.contains(module)
    }
}

impl LogConfig {
    pub fn get_log_level(&self) -> LevelFilter {
        self.log_level.0
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            cache_timeout: Duration::try_hours(48).unwrap().num_seconds(),
            antifloodwait_count: 80,
            antifloodwait_time: 150,
            ignore_chat_time: Duration::try_minutes(10).unwrap().num_seconds(),
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
            modules: Modules::default(),
            persistence: Persistence::default(),
            logging: LogConfig::default(),
            webhook: WebhookConfig::default(),
            timing: Timing::default(),
            admin: Admin::default(),
            compute_threads: num_cpus::get(),
        }
    }
}

// Mildly competent moduler telegram bot
#[derive(Parser, Default, Debug)]
#[clap(author, version, long_about = None)]
pub struct Args {
    // Path to config file
    #[clap(short, long)]
    pub config: PathBuf,
}

lazy_static! {
    pub static ref ME: OnceCell<User> = OnceCell::new();
    pub static ref USERNAME: &'static str = ME.get().unwrap().get_username().unwrap();
    pub static ref AT_HANDLE: String = format!("@{}", *USERNAME);
}

lazy_static! {
    pub static ref EXEC: Runtime = {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(16 * 1024 * 1024)
            .build()
            .expect("create tokio threadpool")
    };
}

//global configuration parameters
lazy_static! {
    pub static ref ARGS: OnceCell<Args> = OnceCell::new();
}

lazy_static! {
    pub(crate) static ref CONFIG_BACKEND: OnceCell<Config> = OnceCell::new();
}

lazy_static! {
    pub static ref CONFIG: &'static Config = CONFIG_BACKEND.get().unwrap();
}

//redis client
#[cfg(not(test))]
lazy_static! {
    pub static ref REDIS_BACKEND: OnceCell<RedisPool<RedisConnectionManager, MultiplexedConnection>> =
        OnceCell::new();
}

#[cfg(not(test))]
lazy_static! {
    pub static ref REDIS: &'static RedisPool<RedisConnectionManager, MultiplexedConnection> =
        REDIS_BACKEND.get().unwrap();
}

//redis client
#[cfg(test)]
lazy_static! {
    pub static ref REDIS_BACKEND: OnceCell<RedisPool<MockPool, redis_test::MockRedisConnection>> =
        OnceCell::new();
}

#[cfg(test)]
lazy_static! {
    pub static ref REDIS: &'static RedisPool<MockPool, redis_test::MockRedisConnection> =
        REDIS_BACKEND.get().unwrap();
}

lazy_static! {
    pub(crate) static ref DB_BACKEND: OnceCell<DatabaseConnection> = OnceCell::new();
}

//db client
lazy_static! {
    pub static ref DB: &'static DatabaseConnection = DB_BACKEND.get().unwrap();
}

lazy_static! {
    pub static ref BAN_GOVERNER: RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware> =
        RateLimiter::direct(Quota::per_second(NonZeroU32::new(30u32).unwrap()));
    pub static ref CHAT_GOVERNER: DefaultKeyedRateLimiter<i64> =
        DefaultKeyedRateLimiter::dashmap(Quota::per_second(NonZeroU32::new(1u32).unwrap()));
}

lazy_static! {
    pub(crate) static ref CLIENT_BACKEND: OnceCell<TgClient> = OnceCell::new();
}

//tg client
lazy_static! {
    pub static ref TG: &'static TgClient = CLIENT_BACKEND.get().unwrap();
}
