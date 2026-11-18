use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=BJUT_BUILD_TIMESTAMP");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../dist");
    let timestamp = match std::env::var("BJUT_BUILD_TIMESTAMP") {
        Ok(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| (1..=253_402_300_799).contains(value))
            .expect("BJUT_BUILD_TIMESTAMP must be a UTC Unix timestamp in seconds"),
        Err(std::env::VarError::NotPresent) => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("build clock predates the Unix epoch")
            .as_secs(),
        Err(error) => panic!("invalid BJUT_BUILD_TIMESTAMP: {error}"),
    };
    println!("cargo:rustc-env=BJUT_BUILD_TIMESTAMP={timestamp}");
    tauri_build::build()
}
