mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn assert_stderr_is_one_json_document(stderr: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stderr).expect("stderr should be exactly one parseable JSON document")
}

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

#[test]
fn json_failure_still_parses_when_a_warning_also_fired() {
    // I1 regression: `warn` used to print `busy: warning: ...` prose to
    // stderr immediately, ahead of the JSON error object, so a run that both
    // warned and failed produced a stderr no JSON parser could read at all.
    // This message is 23 characters in `large`, which the bounds checker
    // flags as too wide for the front panel's 72px — the same fixture the
    // re-verification command in the fix report uses. The address is
    // unreachable, so the run also fails, giving both halves of the bug in
    // one invocation.
    let output = busy()
        .args([
            "--json",
            "--addr",
            "http://127.0.0.1:1",
            "text",
            "Deployment completed OK",
        ])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));

    let value = assert_stderr_is_one_json_document(&output.stderr);
    assert_eq!(value["ok"], serde_json::json!(false));
    assert!(
        value["warnings"][0]
            .as_str()
            .expect("warnings should be an array of strings")
            .contains("clips silently"),
        "got {value:?}"
    );
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
fn transliteration_warns_only_when_something_changed() {
    // I2: README.md promises "a warning is printed" when smart quotes/dashes
    // are transliterated; nothing wired `Sanitized::changed` up to emit one.
    let changed = busy()
        .args(["--dry-run", "text", "don\u{2019}t \u{2014} really"])
        .output()
        .expect("should run");
    assert!(
        changed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&changed.stderr)
    );
    let stderr = String::from_utf8_lossy(&changed.stderr);
    assert!(
        stderr.contains("transliterat"),
        "a changed message should warn, got {stderr:?}"
    );

    let plain = busy()
        .args(["--dry-run", "text", "plain ascii"])
        .output()
        .expect("should run");
    assert!(
        plain.stderr.is_empty(),
        "an unchanged message should not warn, got {:?}",
        plain.stderr
    );
}

/// A temporary `XDG_CONFIG_HOME` holding a `busy/config.toml`, so a test can
/// point `busy` at a config file without touching the developer's real one.
/// No dependency on a tempdir crate: the constraint is no new dependencies.
struct TempConfigHome {
    dir: std::path::PathBuf,
}

impl TempConfigHome {
    fn with_contents(contents: &str) -> Self {
        let unique = format!(
            "busy-cli-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        let config_dir = dir.join("busy");
        std::fs::create_dir_all(&config_dir).expect("should create temp config dir");
        std::fs::write(config_dir.join("config.toml"), contents).expect("should write config");
        Self { dir }
    }
}

impl Drop for TempConfigHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_malformed_config_warns_even_under_quiet() {
    // I4: a broken config is warn-and-continue by design (a typo must not
    // block a notification), but that warning means "your configuration is
    // not being applied" -- categorically different from a routine advisory
    // -- so `--quiet` must not be able to hide it.
    let home = TempConfigHome::with_contents("this is not valid toml [[[");

    let output = busy()
        .env("XDG_CONFIG_HOME", &home.dir)
        .args(["--quiet", "--dry-run", "text", "hi"])
        .output()
        .expect("should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignoring") && stderr.contains("config.toml"),
        "a malformed config must warn even under --quiet, got {stderr:?}"
    );
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
