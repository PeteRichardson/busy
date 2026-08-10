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
fn a_literal_dash_message_is_still_reachable() {
    // `--` terminates option parsing; the value after it is a literal.
    let output = busy()
        .args(["--dry-run", "text", "--", "-3 tests failing"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "-3 tests failing""#));
}
