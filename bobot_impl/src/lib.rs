use std::path::PathBuf;

use async_executors::{TokioTp, TokioTpBuilder};
use clap::Parser;
use lazy_static::lazy_static;

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

pub mod persist;
pub mod tg;
pub(crate) mod util;

pub mod modules;
