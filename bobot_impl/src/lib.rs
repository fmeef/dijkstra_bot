use async_executors::{TokioTp, TokioTpBuilder};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref EXEC: TokioTp = {
        TokioTpBuilder::new()
            .build()
            .expect("create tokio threadpool")
    };
}

pub fn get_executor() -> TokioTp {
    EXEC.clone()
}

pub mod persist;
pub mod util;

pub mod core;
