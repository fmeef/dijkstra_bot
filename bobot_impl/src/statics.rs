use crate::persist::redis::{RedisPool, RedisPoolBuilder};

use super::Args;
use clap::Parser;
use futures::executor::block_on;
use lazy_static::lazy_static;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::env;

//global configuration parameters
lazy_static! {
    pub(crate) static ref ARGS: Args = Args::parse();
    pub(crate) static ref API_ID: i32 = env::var("API_ID")
        .expect("need to set API_ID")
        .parse()
        .expect("invalid API_ID");
    pub(crate) static ref BOT_TOKEN: String = env::var("FMEFTOKEN").expect("need to set FMEFTOKEN");
    pub(crate) static ref API_HASH: String = env::var("API_HASH").expect("need to set API_HASH");
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
    pub(crate) static ref DB: DatabaseConnection = block_on(Database::connect(
        ConnectOptions::new(PG_CONNECTION_STR.clone())
    ))
    .expect("failed to initialize database");
}
