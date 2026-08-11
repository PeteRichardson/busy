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
fn validate_with_no_name_reports_every_broken_template_not_just_the_first() {
    // I4: each step (load/analyse/render/build) used to propagate with `?`,
    // abandoning the loop at the first broken template — a bulk `validate`'s
    // entire point is to check everything before you need it. Two templates
    // broken in different ways, at different steps: `aaa-broken` fails to
    // parse as TOML at all (fails during `Template::render`), `zzz-dup` loads
    // and renders fine but has a duplicate element id (fails during offline
    // validation). Both names must appear, not just whichever sorts first.
    let dir = std::env::temp_dir().join("busy-tpl-validate-both-broken");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("aaa-broken")).expect("temp dir");
    std::fs::write(dir.join("aaa-broken/template.toml"), "not valid toml [[[").expect("write");
    std::fs::create_dir_all(dir.join("zzz-dup")).expect("temp dir");
    std::fs::write(
        dir.join("zzz-dup/template.toml"),
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
        .args(["template", "validate"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("aaa-broken"),
        "should report the first broken template, got {stderr}"
    );
    assert!(
        stderr.contains("zzz-dup"),
        "must not abandon the loop after aaa-broken — should also report the second \
         broken template, got {stderr}"
    );
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

#[test]
fn show_and_init_json_carry_a_summary_key_like_every_other_command() {
    // I5: `template show --json` and `template init --json` omitted
    // `summary`, the one key every other command's `--json` envelope carries
    // (`template list`, `template validate`, `clear --dry-run`, `draw`,
    // `asset list`, …). A wrapper script reading `.summary` from every `busy
    // … --json` invocation got `null` for exactly these two.
    let dir = root("json-summary");

    let show = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--json", "template", "show", "error"])
        .output()
        .expect("should run");
    assert!(show.status.success());
    let value: serde_json::Value = serde_json::from_slice(&show.stdout).expect("valid json");
    assert!(
        value["summary"].is_string(),
        "template show --json must carry a summary, got {value}"
    );

    let init_dir = std::env::temp_dir().join("busy-tpl-json-summary-init");
    let _ = std::fs::remove_dir_all(&init_dir);
    let init = busy()
        .args(["--template-dir"])
        .arg(&init_dir)
        .args(["--json", "template", "init"])
        .output()
        .expect("should run");
    assert!(init.status.success());
    let value: serde_json::Value = serde_json::from_slice(&init.stdout).expect("valid json");
    assert!(
        value["summary"].is_string(),
        "template init --json must carry a summary, got {value}"
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

// I2: `template run` shared `DrawArgs` wholesale, so `--help` advertised
// `--file` (which bypasses templates entirely) and `--as` (silently
// overridden to `template`), and the subcommand had no dedicated test at any
// level (`grep -rn '"run"' tests/` returned nothing). `TemplateRunArgs` now
// carries only the fields `DrawCommon` shares with `DrawArgs`.

#[test]
fn run_renders_and_draws() {
    let dir = root("run");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "template", "run", "plain"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"text\": \"static\""),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn run_with_a_missing_required_variable_exits_2() {
    let dir = root("run-missing");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "template", "run", "error"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error"),
        "should name the template, got {stderr}"
    );
    assert!(
        stderr.contains("message"),
        "should name the variable, got {stderr}"
    );
}

#[test]
fn run_help_does_not_mention_file_or_as() {
    let output = busy()
        .args(["template", "run", "--help"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("--file"), "got {stdout}");
    assert!(!stdout.contains("--as"), "got {stdout}");
}

#[test]
fn run_rejects_file_and_as_as_unknown_flags() {
    // Not just absent from --help: clap must actually refuse them, since
    // `--file` on `run` would bypass templates entirely and `--as` would
    // parse and then be silently overridden.
    let dir = root("run-rejects");
    for flag in [vec!["--file", "/tmp/whatever.json"], vec!["--as", "image"]] {
        let output = busy()
            .args(["--template-dir"])
            .arg(&dir)
            .args(["--dry-run", "template", "run", "plain"])
            .args(&flag)
            .output()
            .expect("should run");
        assert_eq!(output.status.code(), Some(2), "{flag:?} should be rejected");
    }
}

// I3/I6: `-` reads the message from stdin, and `--quiet` on a listing must
// not suppress the listing (matching `asset list`).

#[test]
fn a_dash_reads_the_draw_message_from_stdin() {
    let dir = root("stdin-draw");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "error", "-"])
        .write_stdin("Fix the thing\n")
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(r#""text": "Fix the thing""#),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_dash_reads_the_template_run_message_from_stdin() {
    let dir = root("stdin-run");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "template", "run", "error", "-"])
        .write_stdin("Fix the thing\n")
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(r#""text": "Fix the thing""#),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn quiet_does_not_suppress_template_list_or_show() {
    // I6: `--quiet`'s own help text is "Suppress warnings"; a listing's
    // output is the answer to the question asked, not commentary about the
    // run, exactly like `asset list --quiet` (see tests/asset.rs).
    let dir = root("quiet");

    let list = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--quiet", "template", "list"])
        .output()
        .expect("should run");
    assert!(
        !list.stdout.is_empty(),
        "template list --quiet must still print the listing"
    );

    let show = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--quiet", "template", "show", "error"])
        .output()
        .expect("should run");
    assert!(
        !show.stdout.is_empty(),
        "template show --quiet must still print the description"
    );
}

#[test]
fn golden_payload_for_a_rendered_multi_element_template() {
    // Pins that a template really does produce the same wire bytes a
    // hand-written payload would, including that `rectangle` — an element
    // kind this project does not model — survives untouched.
    let dir = std::env::temp_dir().join("busy-tpl-golden");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("status")).expect("temp dir");
    std::fs::write(
        dir.join("status/template.toml"),
        r#"description = "text plus a progress bar"
priority = 95
[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
x = 2
y = 8
align = "mid_left"
font = "small"
[[elements]]
id = "bar"
type = "rectangle"
width = 40
height = 3
x = 2
y = 14
align = "mid_left"
"#,
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "status", "Deploying"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    insta::assert_snapshot!(String::from_utf8_lossy(&output.stdout));
}
