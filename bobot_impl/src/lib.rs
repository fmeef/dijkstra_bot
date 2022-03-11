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

#[derive(Parser)]
#[clap(author, version, long_about = None)]
struct Args {
    #[clap(short, long)]
    session: String,
}

pub fn get_executor() -> TokioTp {
    EXEC.clone()
}

pub mod persist;
pub mod tg;
pub(crate) mod util;

pub mod core;
pub mod modules;
