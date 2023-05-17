//! Counters and functions for collecting usage metrics and error reporting
//! mainly used with prometheus

use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
//counters
lazy_static! {
    /// map of counters for telegram error codes, lazy initialized, one per http error code
    pub static ref ERROR_CODES_MAP: DashMap<i64, IntCounter> = DashMap::new();
}

/// register a http error code returned from telegra, lazy-initializing a prometheus counter
/// as needed
pub fn count_error_code(err: i64) {
    let counter = ERROR_CODES_MAP.entry(err).or_insert_with(|| {
        register_int_counter!(format! {"errcode_{}", err}, "Telegram error counter").unwrap()
    });
    counter.value().inc();
}
