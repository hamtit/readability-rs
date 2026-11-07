//! Runs the napi-rs setup when the Node-API addon is built.

use std::env;

fn main() {
    if env::var_os("CARGO_FEATURE_NAPI").is_some() {
        napi_build::setup();
    }
}
