mod common;

use common::busy;

/// Run `busy`, require success, and hand back stdout — which under `--dry-run`
/// is the exact wire payload.
fn stdout(args: &[&str]) -> String {
    let output = busy().args(args).output().expect("should run");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be UTF-8")
}

#[test]
fn golden_payload_for_the_fully_specified_command() {
    let payload = stdout(&[
        "--dry-run",
        "text",
        "-x",
        "0",
        "-y",
        "8",
        "--align",
        "mid_left",
        "--font",
        "small",
        "--color",
        "0xFF0000FF",
        "Goodbye, World!",
    ]);
    insta::assert_snapshot!(payload);
}

#[test]
fn golden_payload_for_the_minimal_command() {
    insta::assert_snapshot!(stdout(&["--dry-run", "text", "Hello, World!"]));
}

#[test]
fn golden_payload_with_a_lifetime_and_an_led() {
    insta::assert_snapshot!(stdout(&[
        "--dry-run",
        "text",
        "--timeout",
        "30",
        "--led",
        "red",
        "--priority",
        "urgent",
        "deploy done",
    ]));
}

#[test]
fn smart_quotes_are_sanitized_into_the_payload() {
    let payload = stdout(&["--dry-run", "text", "don\u{2019}t \u{2014} really"]);
    assert!(
        payload.contains(r#""text": "don't - really""#),
        "got {payload}"
    );
}

#[test]
fn a_message_that_sanitizes_to_empty_is_a_clear_error() {
    let output = busy()
        .args(["--dry-run", "text", "\u{1f389}"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nothing printable"),
        "expected a clear message, got {stderr}"
    );
}

#[test]
fn timeout_and_until_conflict() {
    let output = busy()
        .args(["text", "--timeout", "30", "--until", "1900000000", "hi"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
