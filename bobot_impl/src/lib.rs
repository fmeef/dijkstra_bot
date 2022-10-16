use std::path::PathBuf;

use async_executors::{TokioTp, TokioTpBuilder};
use clap::Parser;
use flexi_logger::Logger;
use lazy_static::lazy_static;
use sea_orm::ConnectionTrait;
use statics::*;

lazy_static! {
    pub static ref EXEC: TokioTp = {
        TokioTpBuilder::new()
            .build()
            .expect("create tokio threadpool")
    };
}

// Mildly competent moduler telegram bot
#[derive(Parser)]
#[clap(author, version, long_about = None)]
pub struct Args {
    // Path to mtproto session file
    #[clap(short, long)]
    pub session: PathBuf,
}

pub fn get_executor() -> TokioTp {
    EXEC.clone()
}

pub fn init_db() {
    println!(
        "db initialized, mock: {}",
        &statics::DB.is_mock_connection()
    );
}

pub async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _logger = Logger::try_with_env_or_str("info, my::info::module=trace")?
        .log_to_stderr()
        .write_mode(flexi_logger::WriteMode::Async)
        .start()?;

    TG.run().await?;
    println!("complete");
    log::logger().flush();
    Ok(())
}

pub mod persist;
pub mod tg;
pub(crate) mod util;

pub mod modules;

pub(crate) mod statics;
