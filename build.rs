// use buildhelpers::autoimport;
// use std::fs;
fn main() {
    //     let f = autoimport("./src/modules").to_string();
    //     fs::write("src/modules/mod.rs", f).unwrap();
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let env = std::env::var("DIJKSTRA_STRINGS_DIR").unwrap_or(format!("{}/strings", manifest));
    println!("cargo:rustc-env=DIJKSTRA_STRINGS_DIR={}", env);
}
