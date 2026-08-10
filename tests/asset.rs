mod common;

use common::busy_at;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn listing_reads_the_apps_asset_directory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .and(query_param("path", "/ext/user_assets/busy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "list": [
                {"type": "file", "name": "logo.png", "size": 451},
                {"type": "file", "name": "icon.png", "size": 73}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "list"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("logo.png"), "got {stdout}");
    assert!(stdout.contains("451"), "should show the size, got {stdout}");
}

#[tokio::test]
async fn an_app_with_no_assets_is_not_an_error() {
    // Delete-all removes the directory rather than emptying it, so a 400 here
    // means "no assets", not a failure. Measured on hardware.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "Bad Request"
        })))
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "list"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "a missing directory must not fail");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no assets"),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
