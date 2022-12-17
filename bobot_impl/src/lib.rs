use sea_orm::ConnectionTrait;
use statics::get_executor;

pub mod persist;
pub mod tg;
pub(crate) mod util;

pub mod modules;

mod logger;
pub mod statics;

fn init_db() {
    println!(
        "db initialized, mock: {}",
        &statics::DB.is_mock_connection()
    );
}
pub fn what() {
    let v = get_executor();
    v.block_on(statics::TG.run()).unwrap();
}

pub fn run() {
    let mut handle = logger::setup_log();
    init_db();
    println!("complete");
    handle.join();
}
