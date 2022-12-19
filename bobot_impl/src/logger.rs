use nonblock_logger::{
    log::LevelFilter, BaseConsumer, BaseFilter, BaseFormater, JoinHandle, NonblockLogger,
};

use std::io;

pub fn setup_log() -> JoinHandle {
    let formater = BaseFormater::new().local(true).color(true).level(4);

    let filter = BaseFilter::new()
        .starts_with(true)
        .max_level(LevelFilter::Info);
    let consumer = BaseConsumer::stdout(filter.max_level_get())
        .chain(LevelFilter::Error, io::stderr())
        .unwrap();

    let logger = NonblockLogger::new()
        .formater(formater)
        .filter(filter)
        .and_then(|l| l.consumer(consumer))
        .unwrap();
    logger
        .spawn()
        .map_err(|e| eprintln!("failed to init nonblock_logger: {:?}", e))
        .unwrap()
}
