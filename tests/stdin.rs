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
