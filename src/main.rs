#![deny(rust_2018_idioms)]
#![allow(dead_code)]

mod modules;
use bobot_impl::async_main;

pub fn main() {
    bobot_impl::EXEC.block_on(async_main()).unwrap();
}
