mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_ok(server: &MockServer) {
    Mock::given(path("/api/display/draw"))
        .respond_with(ok())
        .mount(server)
        .await;
}

#[tokio::test]
async fn json_success_is_parseable_and_carries_the_payload() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let output = busy_at(&server)
        .args(["--json", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["ok"], serde_json::json!(true));
    assert_eq!(
        value["payload"]["application_name"],
        serde_json::json!("busy")
    );
    assert_eq!(
        value["payload"]["elements"][0]["text"],
        serde_json::json!("hi")
    );
}

#[tokio::test]
async fn json_failure_is_parseable_and_goes_to_stderr() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(serde_json::json!({"error": "nope"})),
        )
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--json", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));

    let value: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(value["ok"], serde_json::json!(false));
    assert!(value["error"].as_str().unwrap().contains("work session"));
}

#[tokio::test]
async fn quiet_suppresses_the_success_line() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let output = busy_at(&server)
        .args(["--quiet", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "got {:?}", output.stdout);
}

#[tokio::test]
async fn quiet_suppresses_bounds_warnings() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let noisy = busy_at(&server)
        .args(["text", "-x", "500", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&noisy.stderr).contains("outside"));

    let quiet = busy_at(&server)
        .args(["--quiet", "text", "-x", "500", "hi"])
        .output()
        .expect("should run");
    assert!(quiet.stderr.is_empty(), "got {:?}", quiet.stderr);
}

#[test]
fn quiet_does_not_suppress_failure() {
    // --quiet silences the success line and warnings, but a silenced failure
    // would be a trap for CI: the exit code alone doesn't say why it failed.
    let output = busy()
        .args(["--quiet", "--addr", "ftp://nope", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty(), "got {:?}", output.stderr);
}

#[test]
fn dry_run_output_is_unaffected_by_json() {
    // --dry-run already emits the exact wire payload, so --json must not wrap it.
    let plain = busy()
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    let jsonic = busy()
        .args(["--dry-run", "--json", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(plain.stdout, jsonic.stdout);
}
