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

    // The counts above only prove one of each arrived; a POST-then-DELETE
    // regression would pass them too, and would wipe the element it just
    // drew. Pin the order explicitly.
    let received = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].method, http::Method::DELETE);
    assert_eq!(received[1].method, http::Method::POST);
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

    // .expect(0) above already proves no DELETE arrived; spell out what did
    // arrive so the test reads as "only a POST happened" rather than just
    // "no DELETE happened".
    let received = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, http::Method::POST);
}

#[tokio::test]
async fn a_plain_draw_of_an_asset_clears_first() {
    // `a_plain_draw_clears_first` above only exercises `busy text`; nothing
    // pinned the same DELETE-then-POST ordering for `busy draw`. If it ever
    // inverted, `busy draw` would wipe the element it had just drawn and
    // still exit 0.
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
        .args(["draw", "logo.png"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // MockServer verifies the .expect() counts when it drops.

    let received = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].method, http::Method::DELETE);
    assert_eq!(received[1].method, http::Method::POST);
}

#[tokio::test]
async fn keep_skips_the_clear_for_draw() {
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
        .args(["draw", "--keep", "logo.png"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let received = server
        .received_requests()
        .await
        .expect("request recording is enabled by default");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, http::Method::POST);
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
