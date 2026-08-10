mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Let the clear succeed and the draw fail, which is the shape every error
/// case here needs.
async fn draw_responds(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_409_becomes_priority_guidance_not_a_status_code() {
    let server = MockServer::start().await;
    draw_responds(
        &server,
        409,
        serde_json::json!({"error": "Requested priority level is below that of currently active app."}),
    )
    .await;

    let output = busy_at(&server)
        .args(["text", "Build Failed!"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("work session"), "got {stderr}");
    assert!(stderr.contains("--priority 95"), "got {stderr}");
    assert!(stderr.contains("config.toml"), "got {stderr}");
    assert!(
        !stderr.contains("409"),
        "the raw status should not lead: {stderr}"
    );
}

#[tokio::test]
async fn the_reported_priority_is_the_one_that_was_requested() {
    let server = MockServer::start().await;
    draw_responds(&server, 409, serde_json::json!({"error": "nope"})).await;

    let output = busy_at(&server)
        .args(["text", "--priority", "low", "hi"])
        .output()
        .expect("should run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("priority 10"), "got {stderr}");
}

#[tokio::test]
async fn a_401_explains_the_access_key() {
    let server = MockServer::start().await;
    draw_responds(&server, 401, serde_json::json!({"error": "Unauthorized"})).await;

    let output = busy_at(&server)
        .args(["text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BUSY_TOKEN"), "got {stderr}");
}

#[tokio::test]
async fn a_400_surfaces_the_device_message() {
    let server = MockServer::start().await;
    draw_responds(
        &server,
        400,
        serde_json::json!({"error": "Failed to decode image /ext/user_assets/busy/nope.png."}),
    )
    .await;

    let output = busy_at(&server)
        .args(["text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to decode image"), "got {stderr}");
}

#[test]
fn a_bad_address_is_a_usage_error() {
    let output = busy()
        .args(["--addr", "ftp://nope", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--addr"));
}
