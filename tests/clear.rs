mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn clear_deletes_this_apps_elements() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "busy"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server).arg("clear").output().expect("should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn clear_is_scoped_to_the_selected_app() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "ci"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--app", "ci", "clear"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn clear_honours_dry_run() {
    let server = MockServer::start().await;
    // No mocks: a request would 404 and fail.
    let output = busy_at(&server)
        .args(["--dry-run", "clear"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("clear"));
}

#[test]
fn clear_dry_run_never_connects_even_to_a_bad_address() {
    // I3: `Device::connect` used to run before the dry-run check, so a bad
    // `--addr` exited 2 for `clear` while `text --dry-run` exited 0 for the
    // same address. `--dry-run` must be the same "contacts nothing, validates
    // nothing external" escape hatch for both commands.
    let output = busy()
        .args(["--dry-run", "--addr", "ftp://nope", "clear"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn clear_json_is_parseable() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--json", "clear"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["ok"], serde_json::json!(true));
}

#[tokio::test]
async fn clear_quiet_is_silent_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--quiet", "clear"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "got {:?}", output.stdout);
}
