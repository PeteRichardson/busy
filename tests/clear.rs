mod common;

use common::{busy_at, ok};
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
