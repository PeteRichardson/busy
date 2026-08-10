mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn a_plain_draw_clears_first() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
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
        .args(["text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    // MockServer verifies the .expect() counts when it drops.
}

#[tokio::test]
async fn keep_skips_the_clear() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["text", "--keep", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[test]
fn the_default_element_id_is_message() {
    let output = busy()
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""id": "message""#));
}

#[test]
fn the_element_id_is_overridable() {
    let output = busy()
        .args(["--dry-run", "text", "--id", "status-line", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""id": "status-line""#));
}

#[test]
fn an_element_id_with_a_space_is_rejected() {
    let output = busy()
        .args(["--dry-run", "text", "--id", "status line", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--id"));
}
