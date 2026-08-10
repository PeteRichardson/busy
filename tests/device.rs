mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn a_draw_reaches_the_device() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "busy"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["text", "Hello, World!"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn the_cloud_prefix_is_selectable() {
    let server = MockServer::start().await;
    Mock::given(path("/busybar/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--api-prefix", "cloud", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn a_token_is_sent_as_a_bearer_header() {
    let server = MockServer::start().await;
    Mock::given(path("/api/display/draw"))
        .and(header("authorization", "Bearer 12345678"))
        .respond_with(ok())
        .expect(2) // the DELETE and the POST
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--token", "12345678", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn dry_run_sends_nothing() {
    let server = MockServer::start().await;
    // No mocks mounted: any request would 404 and fail the command.
    let output = busy_at(&server)
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"application_name\": \"busy\""));
}

#[tokio::test]
async fn an_unreachable_device_fails_with_exit_1() {
    let output = busy()
        .args(["--addr", "http://127.0.0.1:1", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("127.0.0.1:1"), "got {stderr}");
}

// D7: `Env::from_process` (the flags -> env -> file -> defaults chain's
// "env" link) had no end-to-end coverage -- every test harness scrubs
// `BUSY_*` via `tests/common`, and the unit tests in `config.rs` build `Env`
// literally rather than reading the process environment. These two use
// `Command::env`, not `std::env::set_var`, so the suite stays parallel-safe.

#[tokio::test]
async fn busy_addr_env_var_is_honoured() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    // No `--addr` flag: only `BUSY_ADDR` points at the mock server. If the
    // env layer were not wired up, this would fall back to the built-in
    // default address and fail to reach the mock.
    let output = busy()
        .env("BUSY_ADDR", server.uri())
        .args(["text", "hi"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn an_explicit_addr_flag_overrides_the_busy_addr_env_var() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    // BUSY_ADDR points at an unreachable address; only the `--addr` flag
    // points at the mock. If the flag did not win, this would fail.
    let output = busy()
        .env("BUSY_ADDR", "http://127.0.0.1:1")
        .args(["--addr", &server.uri(), "text", "hi"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
