mod common;

use common::busy;

#[test]
fn a_dash_reads_the_message_from_stdin() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("Build failed\n")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(r#""text": "Build failed""#),
        "trailing newline should be trimmed"
    );
}

#[test]
fn only_the_final_newline_is_trimmed() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("a  b\n")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "a  b""#));
}

#[test]
fn empty_stdin_is_a_clear_error() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("stdin"));
}

#[test]
fn empty_stdin_on_a_draw_does_not_blame_busy_text() {
    // `-` reads stdin on `draw`/`template run` too now, so the empty-stdin
    // message must not name `busy text` specifically — it used to, back when
    // `busy text -` was the only path that could ever reach it.
    let dir = std::env::temp_dir().join("busy-stdin-empty-draw");
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
        .args(["--dry-run", "draw", "say", "-"])
        .write_stdin("")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdin"), "got {stderr}");
    assert!(
        !stderr.contains("busy text"),
        "must not name a command the user did not run, got {stderr}"
    );
}

#[test]
fn a_message_starting_with_a_dash_is_reachable_after_a_double_dash() {
    // `--` terminates option parsing, so clap accepts a value starting with
    // `-` instead of trying to parse it as a flag cluster. This is not the
    // same as making a *bare* `-` reachable as a literal message — see
    // `a_bare_dash_after_a_double_dash_still_reads_stdin` below.
    let output = busy()
        .args(["--dry-run", "text", "--", "-3 tests failing"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "-3 tests failing""#));
}

#[test]
fn a_bare_dash_after_a_double_dash_still_reads_stdin() {
    // `--` only stops clap from treating the following token as a flag; it
    // does not change the resolved string value handed to `read_message`.
    // A bare `-` is still exactly `"-"` whether or not `--` preceded it, so
    // it still hits the stdin sentinel rather than becoming a literal
    // one-character message. This pins that documented limitation (see
    // `src/input.rs`) as an executable fact.
    let output = busy()
        .args(["--dry-run", "text", "--", "-"])
        .write_stdin("piped content\n")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "piped content""#));
}
