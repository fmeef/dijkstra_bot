use crate::persist::redis::{RedisPool, RedisPoolBuilder};
use crate::tg::client::TgClient;
use async_executors::{TokioTp, TokioTpBuilder};
use chrono::Duration;
use clap::Parser;
use confy::load_path;
use futures::executor::block_on;
use lazy_static::lazy_static;
use sea_orm::entity::prelude::DatabaseConnection;
use sea_orm::{ConnectOptions, Database};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;

#[derive(Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) cache_timeout: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cache_timeout: Duration::hours(48).num_seconds() as usize,
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
    pub static ref EXEC: TokioTp = {
        TokioTpBuilder::new()
            .build()
            .expect("create tokio threadpool")
    };
}

//global configuration parameters
lazy_static! {
    pub(crate) static ref ARGS: Args = Args::parse();
    pub(crate) static ref CONFIG: Config = load_path(&ARGS.config).expect("failed to load config");
    pub(crate) static ref BOT_TOKEN: String = env::var("TOKEN").expect("need to set FMEFTOKEN");
    pub(crate) static ref PG_CONNECTION_STR: String =
        env::var("PG_CONNECTION_PROD").expect("need to set PG_CONNECTION_PROD");
    pub(crate) static ref REDIS_CONNECTION_STR: String =
        env::var("REDIS_CONNECTION_PROD").expect("need to set REDIS_CONNECTION_PROD");
}

//redis client
lazy_static! {
    pub(crate) static ref REDIS: RedisPool =
        block_on(RedisPoolBuilder::new(REDIS_CONNECTION_STR.clone()).build())
            .expect("failed to initialize redis pool");
}

//db client
lazy_static! {
    pub(crate) static ref DB: Arc<DatabaseConnection> =
        Runtime::new().unwrap().block_on(async move {
            let db = Database::connect(ConnectOptions::new(PG_CONNECTION_STR.clone()))
                .await
                .expect("failed to initialize database");
            Arc::new(db)
        });
}

//tg client
lazy_static! {
    pub(crate) static ref TG: TgClient = TgClient::connect(BOT_TOKEN.clone());
}

pub fn get_executor() -> TokioTp {
    EXEC.clone()
}
