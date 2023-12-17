fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let env = std::env::var("DIJKSTRA_STRINGS_DIR").unwrap_or(format!("{}/strings", manifest));
    println!("cargo:rustc-env=DIJKSTRA_STRINGS_DIR={}", env);
}
