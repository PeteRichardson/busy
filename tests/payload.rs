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

#[test]
fn text_until_still_produces_a_display_until_field() {
    // Pinned by issue #12: splitting `draw`'s delivery args away from
    // `text`'s must not disturb `--until` on `text` itself.
    let payload = stdout(&["--dry-run", "text", "--until", "2027-01-01T00:00:00Z", "hi"]);
    assert!(payload.contains("\"display_until\""), "got {payload}");
}

#[test]
fn until_accepts_unix_seconds() {
    let payload = stdout(&["--dry-run", "text", "--until", "1900000000", "hi"]);
    // `Lifetime` is `#[serde(untagged)]` and flattened onto the element, and
    // `display_until` is written through `serde_util::string_u64`, so it
    // appears as a *string*, not a bare number.
    assert!(
        payload.contains(r#""display_until": "1900000000""#),
        "got {payload}"
    );
}

#[test]
fn until_accepts_rfc_3339_and_agrees_with_the_equivalent_unix_seconds() {
    // 1900000000 == 2030-03-17T17:46:40Z; making the two forms agree is a
    // stronger check than asserting on either one in isolation.
    let from_unix_seconds = stdout(&["--dry-run", "text", "--until", "1900000000", "hi"]);
    let from_rfc_3339 = stdout(&["--dry-run", "text", "--until", "2030-03-17T17:46:40Z", "hi"]);
    assert_eq!(from_unix_seconds, from_rfc_3339);
    assert!(
        from_rfc_3339.contains(r#""display_until": "1900000000""#),
        "got {from_rfc_3339}"
    );
}

#[test]
fn until_before_1970_is_rejected() {
    let output = busy()
        .args(["--dry-run", "text", "--until", "1960-01-01T00:00:00Z", "hi"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--until") && stderr.contains("1970"),
        "got {stderr}"
    );
}

#[test]
fn a_malformed_until_names_the_flag_and_the_accepted_forms() {
    let output = busy()
        .args(["--dry-run", "text", "--until", "nonsense", "hi"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--until") && stderr.contains("RFC 3339") && stderr.contains("Unix"),
        "got {stderr}"
    );
}
