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

/// Run `busy`, require success, and hand back stdout.
fn stdout(args: &[&str]) -> Vec<u8> {
    let output = busy().args(args).output().expect("should run");
    assert!(
        output.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn every_short_option_resolves_to_its_long_form() {
    // Comparing the two payloads byte-for-byte is what pins the mapping: a
    // future reshuffle that swapped, say, -s and -r would still parse, still
    // exit 0, and only this equality would notice.
    let long = stdout(&[
        "--dry-run",
        "text",
        "--color",
        "red",
        "--font",
        "small",
        "-x",
        "4",
        "-y",
        "2",
        "--align",
        "mid_left",
        "--screen",
        "front",
        "--width",
        "60",
        "--scroll-rate",
        "600",
        "--priority",
        "high",
        "--timeout",
        "30",
        "--led",
        "blue",
        "--id",
        "note",
        "hi",
    ]);

    let short = stdout(&[
        "-n", "text", "-c", "red", "-f", "small", "-x", "4", "-y", "2", "-a", "mid_left", "-s",
        "front", "-w", "60", "-r", "600", "-p", "high", "-t", "30", "-l", "blue", "-i", "note",
        "hi",
    ]);

    assert_eq!(
        String::from_utf8_lossy(&long),
        String::from_utf8_lossy(&short)
    );
}

#[test]
fn the_boolean_short_flags_are_accepted() {
    // -k and -j carry no payload of their own, so they cannot be pinned by the
    // equivalence test above; assert they parse and mean what the long forms mean.
    assert_eq!(
        stdout(&["--dry-run", "--json", "text", "--keep", "hi"]),
        stdout(&["-n", "-j", "text", "-k", "hi"])
    );
}

#[test]
fn the_connection_globals_are_deliberately_long_only() {
    // A global short is reserved across every present and future subcommand, so
    // the rarely-typed connection flags stay long-only. -t is the per-invocation
    // element timeout, deliberately not --token (which would put a secret in
    // shell history and `ps`) nor --http-timeout.
    busy()
        .args(["--dry-run", "text", "-t", "30", "hi"])
        .assert()
        .success();

    for absent in [["-A", "http://x"], ["-T", "12345678"], ["-P", "device"]] {
        busy().args(absent).args(["text", "hi"]).assert().failure();
    }
}
