use std::path::PathBuf;

use async_executors::{TokioTp, TokioTpBuilder};
use chrono::Duration;
use clap::Parser;
use lazy_static::lazy_static;
use nonblock_logger::{
    log::LevelFilter, BaseConsumer, BaseFilter, BaseFormater, JoinHandle, NonblockLogger,
};
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use statics::*;
use std::{io};

lazy_static! {
    pub static ref EXEC: TokioTp = {
        TokioTpBuilder::new()
            .build()
            .expect("create tokio threadpool")
    };
}

fn log() -> JoinHandle {
    let formater = BaseFormater::new().local(true).color(true).level(4);
    println!("{:?}", formater);

    let filter = BaseFilter::new()
        .starts_with(true)
        .chain("logs", LevelFilter::Trace)
        .chain("logt", LevelFilter::Off);
    println!("{:?}", filter);

    let consumer = BaseConsumer::stdout(filter.max_level_get())
        .chain(LevelFilter::Error, io::stderr())
        .unwrap();
    println!("{:?}", consumer);

    let logger = NonblockLogger::new()
        .formater(formater)
        .filter(filter)
        .and_then(|l| l.consumer(consumer))
        .unwrap();

    println!("{:?}", logger);

    logger
        .spawn()
        .map_err(|e| eprintln!("failed to init nonblock_logger: {:?}", e))
        .unwrap()
}

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
    let mut handle = log();
    TG.run().await?;
    println!("complete");
    handle.join();
    Ok(())
}

pub mod persist;
pub mod tg;
pub(crate) mod util;

pub mod modules;

pub(crate) mod statics;
