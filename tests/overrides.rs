mod common;

use common::busy;

fn root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("busy-ovr-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("two")).expect("temp dir");
    std::fs::write(
        dir.join("two/template.toml"),
        r#"priority = 40
[[elements]]
id = "a"
type = "text"
text = "one"
font = "small"
[[elements]]
id = "b"
type = "text"
text = "two"
font = "small"
"#,
    )
    .expect("write");
    dir
}

#[test]
fn payload_level_flags_override_a_template() {
    let dir = root("payload");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args([
            "--dry-run",
            "draw",
            "two",
            "--priority",
            "high",
            "--led",
            "blue",
        ])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"priority\": 95"), "got {stdout}");
    assert!(stdout.contains("led_notification_color"), "got {stdout}");
}

#[test]
fn an_absent_flag_leaves_the_templates_own_value_alone() {
    // Substituting a default here would silently overwrite a value the
    // template never asked to have replaced.
    let dir = root("absent");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "two"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"priority\": 40"),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn per_element_flags_are_rejected_on_a_template() {
    let dir = root("perelement");
    for flag in [
        vec!["-x", "4"],
        vec!["-y", "2"],
        vec!["--align", "center"],
        vec!["--screen", "back"],
        vec!["--timeout", "30"],
        vec!["--opacity", "50"],
        vec!["--id", "mine"],
    ] {
        let output = busy()
            .args(["--template-dir"])
            .arg(&dir)
            .args(["--dry-run", "draw", "two"])
            .args(&flag)
            .output()
            .expect("should run");
        assert_eq!(output.status.code(), Some(2), "{flag:?} should be rejected");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(flag[0]),
            "error must name {}, got {stderr}",
            flag[0]
        );
    }
}

#[test]
fn var_is_rejected_on_a_non_template_draw() {
    // A flag that parses but does nothing is a defect.
    let output = busy()
        .args(["--dry-run", "draw", "logo.png", "--var", "a=1"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--var"));
}

#[test]
fn a_second_positional_on_an_image_draw_names_the_near_match() {
    let dir = root("typo");
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(
        dir.join("error/template.toml"),
        "[[elements]]\nid = \"m\"\ntype = \"text\"\ntext = \"{{ message }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "eror", "Build failed"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Did you mean `error`"), "got {stderr}");
}

// The two tests below close the gap carried forward from Task 5:
// `bind_variables`/`sanitize_values` had unit coverage, but no CLI path fed a
// real `--var` or positional value through it before `draw`/`template run`
// existed. `template validate`'s placeholder path only ever binds the
// literal `"x"`, so it can never exercise the sanitize-and-warn branch either
// (see `tests/template.rs`'s
// `a_literal_smart_quote_in_the_template_text_still_hard_fails_naming_the_template`).
// These are the first tests that send a real smart quote through `--var` and
// through the `message` positional and check the wire payload.

#[test]
fn a_smart_quote_in_the_positional_message_is_sanitized_and_warned_about() {
    let dir = std::env::temp_dir().join("busy-ovr-quote-positional");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("say")).expect("temp dir");
    std::fs::write(
        dir.join("say/template.toml"),
        "[[elements]]\nid = \"a\"\ntype = \"text\"\ntext = \"{{ message }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "say", "It\u{2019}s done"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"text\": \"It's done\""), "got {stdout}");
    assert!(
        stdout.is_ascii(),
        "the drawn payload must be plain ASCII, got {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("transliterated"),
        "should warn about the substitution, got {stderr}"
    );
}

#[test]
fn a_smart_quote_in_a_var_is_sanitized_and_warned_about() {
    let dir = std::env::temp_dir().join("busy-ovr-quote-var");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("note")).expect("temp dir");
    std::fs::write(
        dir.join("note/template.toml"),
        "[[elements]]\nid = \"a\"\ntype = \"text\"\ntext = \"{{ note }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args([
            "--dry-run",
            "draw",
            "note",
            "--var",
            "note=It\u{2019}s done",
        ])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"text\": \"It's done\""), "got {stdout}");
    assert!(
        stdout.is_ascii(),
        "the drawn payload must be plain ASCII, got {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("transliterated"),
        "should warn about the substitution, got {stderr}"
    );
}

// Fix round 1: a missing required variable used to surface as minijinja's
// raw `undefined value (in <name>:<line>)`, which names neither the
// variable nor how to supply it — even though `render::analyse` already
// knows what's required, and `bind_variables` already knows what was
// supplied. These two are the CLI-level proof that the comparison actually
// runs before rendering and produces a real message; unit coverage for the
// wording itself lives in `src/template/mod.rs`.

#[test]
fn a_missing_required_variable_names_the_template_and_the_variable_via_the_cli() {
    let dir = std::env::temp_dir().join("busy-ovr-missing-message");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("greet")).expect("temp dir");
    std::fs::write(
        dir.join("greet/template.toml"),
        "[[elements]]\nid = \"a\"\ntype = \"text\"\ntext = \"{{ message }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "greet"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("greet"),
        "should name the template, got {stderr}"
    );
    assert!(
        stderr.contains("message"),
        "should name the variable, got {stderr}"
    );
    assert!(
        !stderr.contains("undefined value"),
        "must not be minijinja's raw message, got {stderr}"
    );
}

#[test]
fn two_missing_required_variables_are_both_named_via_the_cli() {
    let dir = std::env::temp_dir().join("busy-ovr-missing-two");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("both")).expect("temp dir");
    std::fs::write(
        dir.join("both/template.toml"),
        "[[elements]]\nid = \"a\"\ntype = \"text\"\ntext = \"{{ first }} {{ second }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "both"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("first"), "got {stderr}");
    assert!(stderr.contains("second"), "got {stderr}");
}
