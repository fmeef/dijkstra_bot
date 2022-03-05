#![deny(rust_2018_idioms)]
#![allow(dead_code)]
use std::env;

async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _token = env::var("FMEFTOKEN").expect("need to set telegram bot token");
    let _pg_connection_str =
        env::var("PG_CONNECTION_PROD").expect("need to set PG_CONNECTION_PROD");
    let _redis_connection_str =
        env::var("REDIS_CONNECTION_PROD").expect("need to set REDIS_CONNECTION_PROD");

    Ok(())
}

pub fn main() {
    bobot_impl::EXEC.block_on(async_main()).unwrap();
}
