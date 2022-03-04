#![deny(rust_2018_idioms)]
#![allow(dead_code)]
use std::env;
mod persist;
mod util;
use async_executors::{TokioTp, TokioTpBuilder};
use lazy_static::lazy_static;

lazy_static! {
    static ref EXEC: TokioTp = {
        TokioTpBuilder::new()
            .build()
            .expect("create tokio threadpool")
    };
}

pub fn get_executor() -> TokioTp {
    EXEC.clone()
}

async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _token = env::var("FMEFTOKEN").expect("need to set telegram bot token");
    let _pg_connection_str =
        env::var("PG_CONNECTION_PROD").expect("need to set PG_CONNECTION_PROD");
    let _redis_connection_str =
        env::var("REDIS_CONNECTION_PROD").expect("need to set REDIS_CONNECTION_PROD");

    Ok(())
}

fn main() {
    EXEC.block_on(async_main()).unwrap();
}
