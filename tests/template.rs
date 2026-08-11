mod common;

use common::busy;
use wiremock::matchers::{method, path as mock_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A template root with `error` (a required variable) and `plain` (none).
fn root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("busy-tpl-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(
        dir.join("error/template.toml"),
        r##"description = "Red error text"
[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
font = "small"
color = "#ff0000ff"
"##,
    )
    .expect("write");
    std::fs::create_dir_all(dir.join("plain")).expect("temp dir");
    std::fs::write(
        dir.join("plain/template.toml"),
        r#"description = "No variables"
[[elements]]
id = "message"
type = "text"
text = "static"
font = "small"
"#,
    )
    .expect("write");
    dir
}

#[test]
fn list_names_every_template_with_its_description() {
    let dir = root("list");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "list"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "got {stdout}");
    assert!(stdout.contains("Red error text"), "got {stdout}");
    assert!(stdout.contains("plain"), "got {stdout}");
}

#[test]
fn show_reports_the_required_variables() {
    let dir = root("show");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "show", "error"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("message"),
        "should name the variable, got {stdout}"
    );
}

#[test]
fn show_of_a_misspelled_name_suggests_the_near_match() {
    let dir = root("suggest");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "show", "eror"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Did you mean `error`"), "got {stderr}");
}

#[test]
fn validate_accepts_a_good_template() {
    let dir = root("valid");
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate"])
        .assert()
        .success();
}

#[test]
fn validate_rejects_a_duplicate_element_id() {
    let dir = std::env::temp_dir().join("busy-tpl-dup");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("dup")).expect("temp dir");
    std::fs::write(
        dir.join("dup/template.toml"),
        r#"[[elements]]
id = "a"
type = "text"
text = "one"
font = "small"
[[elements]]
id = "a"
type = "text"
text = "two"
font = "small"
"#,
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate", "dup"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("duplicate element id"),
        "got {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Addition to the Task 5 brief: a `--var`/positional variable value with a
/// smart quote is sanitized (see the unit tests in `src/template/mod.rs` for
/// the render-succeeds-with-a-warning half, which has no CLI-reachable path
/// in this task — `template validate`'s placeholder is always the literal
/// `"x"`, and `run`/`draw --var` aren't wired up until Task 7). This test
/// covers the other half, which *is* CLI-reachable today: literal non-ASCII
/// in the template file's own text is a template-author problem, not a
/// caller-supplied-value problem, and must keep hard-failing rather than
/// being silently transliterated.
#[test]
fn a_literal_smart_quote_in_the_template_text_still_hard_fails_naming_the_template() {
    let dir = std::env::temp_dir().join("busy-tpl-literal-non-ascii");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("smartquote")).expect("temp dir");
    std::fs::write(
        dir.join("smartquote/template.toml"),
        "[[elements]]\nid = \"a\"\ntype = \"text\"\ntext = \"It\u{2019}s done\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate", "smartquote"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("smartquote"),
        "should name the template, got {stderr}"
    );
}

/// Fix round 1: `template list --json` used to fold the listing into a
/// pre-formatted `summary` string (a `name\tdescription\n...` blob), the
/// same defect `asset list --json` had before `Emitter::success_list` (now
/// generalized to `success_items`) replaced it with an addressable array.
/// Parses the emitted JSON and asserts on structure, not a substring of the
/// blob, since substring-matching the old shape (`stdout.contains("error")`,
/// `stdout.contains("Red error text")`, in
/// `list_names_every_template_with_its_description` above) is exactly what
/// let that shape through unnoticed.
#[test]
fn list_json_carries_an_addressable_templates_array() {
    let dir = root("json-list");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--json", "template", "list"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid json");
    let templates = value["templates"]
        .as_array()
        .expect("templates array present");
    assert_eq!(templates.len(), 2);

    let error = templates
        .iter()
        .find(|entry| entry["name"] == "error")
        .expect("the `error` template should be listed");
    assert_eq!(error["description"], "Red error text");

    let plain = templates
        .iter()
        .find(|entry| entry["name"] == "plain")
        .expect("the `plain` template should be listed");
    assert_eq!(plain["description"], "No variables");
}

/// `template list --json` against an empty root still carries the `templates`
/// key, `[]` rather than absent — the same "always present" contract
/// `asset list --json` gives `assets`, so a consumer never has to branch on
/// whether the key exists.
#[test]
fn list_json_carries_an_empty_templates_array_when_there_is_nothing() {
    let dir = std::env::temp_dir().join("busy-tpl-json-list-empty");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--json", "template", "list"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid json");
    assert_eq!(
        value["templates"],
        serde_json::json!([]),
        "the templates key must be present and empty, not absent, got {value}"
    );
}

/// `template show --json` used to fold everything into one `summary` blob
/// too. Now it must carry structured fields a consumer can address directly.
#[test]
fn show_json_carries_structured_fields() {
    let dir = root("json-show");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--json", "template", "show", "error"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid json");
    assert_eq!(value["name"], "error");
    assert_eq!(value["description"], "Red error text");
    assert_eq!(value["elements"], 1);
    let variables = value["variables"]
        .as_array()
        .expect("variables should be an array");
    assert_eq!(variables, &vec![serde_json::json!("message")]);
    assert!(
        value["path"]
            .as_str()
            .expect("path should be a string")
            .contains("error"),
        "got {value}"
    );
}

#[tokio::test]
async fn a_template_referencing_an_absent_asset_names_the_upload_command() {
    let dir = std::env::temp_dir().join("busy-tpl-asset");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logo")).expect("temp dir");
    std::fs::write(
        dir.join("logo/template.toml"),
        "[[elements]]\nid = \"i\"\ntype = \"image\"\npath = \"stop.png\"\n",
    )
    .expect("write");
    // The local file exists, so offline validation passes; the device does not
    // have it, which is what this check is for.
    std::fs::write(dir.join("logo/stop.png"), b"not really a png").expect("write");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(mock_path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"list": []})))
        .mount(&server)
        .await;

    let output = common::busy_at(&server)
        .args(["--template-dir"])
        .arg(&dir)
        .args(["draw", "logo"])
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stop.png"), "got {stderr}");
    assert!(
        stderr.contains("busy asset upload"),
        "must name the fix, got {stderr}"
    );
}

#[tokio::test]
async fn a_failed_listing_does_not_block_the_draw() {
    // `/ext/user_assets/<app>/` is undocumented. A firmware change must not
    // break the tool; it may only make the resulting error later and worse.
    let dir = std::env::temp_dir().join("busy-tpl-degrade");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logo")).expect("temp dir");
    std::fs::write(
        dir.join("logo/template.toml"),
        "[[elements]]\nid = \"i\"\ntype = \"image\"\npath = \"stop.png\"\n",
    )
    .expect("write");
    std::fs::write(dir.join("logo/stop.png"), b"not really a png").expect("write");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(mock_path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(common::ok())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(mock_path("/api/display/draw"))
        .respond_with(common::ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = common::busy_at(&server)
        .args(["--template-dir"])
        .arg(&dir)
        .args(["draw", "logo"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "a failed listing must not block: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
