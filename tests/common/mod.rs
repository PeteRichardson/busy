//! Helpers shared by the integration test crates.
//!
//! Each file in tests/ is a separate crate that compiles this whole module but
//! uses only part of it, so unused-item warnings here are expected rather than
//! a signal.
#![allow(dead_code)]

use assert_cmd::Command;
use wiremock::{MockServer, ResponseTemplate};

/// A `busy` invocation with a neutral environment, so a developer's own config
/// file and `BUSY_*` variables can never change what a test observes.
pub fn busy() -> Command {
    let mut command = Command::cargo_bin("busy").expect("binary `busy` should build");
    command
        .env_remove("BUSY_ADDR")
        .env_remove("BUSY_TOKEN")
        .env_remove("BUSY_APP")
        .env("XDG_CONFIG_HOME", "/nonexistent");
    command
}

/// `busy` pointed at a mock device.
pub fn busy_at(server: &MockServer) -> Command {
    let mut command = busy();
    command.args(["--addr", &server.uri()]);
    command
}

/// The body the device returns on success.
pub fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": "OK"}))
}
