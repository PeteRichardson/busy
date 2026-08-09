mod common;

use common::busy;
use predicates::str::contains;

#[test]
fn bare_invocation_prints_help_and_fails() {
    // Verified against clap 4.6: `arg_required_else_help` writes the help to
    // stderr and exits 2.
    busy()
        .assert()
        .failure()
        .code(2)
        .stderr(contains("Usage: busy"));
}

#[test]
fn text_without_a_message_is_a_usage_error() {
    busy().arg("text").assert().failure().code(2);
}

#[test]
fn text_with_a_message_parses() {
    // --dry-run is parsed but ignored until Task 5, and never contacts a device
    // in any task — which is what keeps this assertion stable as the tool grows.
    busy()
        .args(["--dry-run", "text", "Hello, World!"])
        .assert()
        .success();
}

#[test]
fn there_is_no_message_flag() {
    busy()
        .args(["text", "-m", "Hello"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn there_is_no_bare_top_level_positional() {
    busy().arg("Hello, World!").assert().failure().code(2);
}
