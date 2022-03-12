#![deny(rust_2018_idioms)]
#![allow(dead_code)]
use clap::Parser;
use std::env;

use bobot_impl::Args;

const SESSION_FILE: &str = "/tmp/whatever.session"; //TODO: change this

async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _token = env::var("FMEFTOKEN").expect("need to set telegram bot token");
    let _api_id: i32 = env::var("API_ID")
        .expect("need to set API_ID")
        .parse()
        .expect("invalid API_ID");
    let _api_hash = env::var("API_HASH").expect("need to set API_HASH");
    let _pg_connection_str =
        env::var("PG_CONNECTION_PROD").expect("need to set PG_CONNECTION_PROD");
    let _redis_connection_str =
        env::var("REDIS_CONNECTION_PROD").expect("need to set REDIS_CONNECTION_PROD");

    let args = Args::parse();

    let client =
        bobot_impl::tg::client::TgClient::connect(_token, _api_id, _api_hash, args.session).await?;

    client.run().await?;
    Ok(())
}

pub fn main() {
    bobot_impl::EXEC.block_on(async_main()).unwrap();
}
