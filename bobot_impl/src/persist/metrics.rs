use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};
//counters
lazy_static! {
    pub static ref TEST_COUNTER: IntCounter =
        register_int_counter!("testlabel", "testhelp").unwrap();
    pub static ref ERROR_CODES: Histogram =
        register_histogram!("module_fails", "Telegram api cries").unwrap();
    pub static ref ERROR_CODES_MAP: DashMap<i64, IntCounter> = DashMap::new();
}

pub fn count_error_code(err: i64) {
    let counter = ERROR_CODES_MAP.entry(err).or_insert_with(|| {
        register_int_counter!(format! {"errcode_{}", err}, "Telegram error counter").unwrap()
    });
    counter.value().inc();
}
