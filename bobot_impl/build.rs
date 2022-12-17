use macros::autoimport;
use std::fs;
fn main() {
    let f = autoimport("./src/modules").to_string();
    // fs::write("src/modules/mod.rs", f).unwrap();
}
