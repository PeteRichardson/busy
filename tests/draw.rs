mod common;

use common::busy;

fn stdout(args: &[&str]) -> String {
    let output = busy().args(args).output().expect("should run");
    assert!(
        output.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be UTF-8")
}

#[test]
fn golden_payload_for_an_asset_with_opacity() {
    insta::assert_snapshot!(stdout(&[
        "--dry-run",
        "draw",
        "logo.png",
        "--opacity",
        "50"
    ]));
}

#[test]
fn golden_payload_for_a_stock_path() {
    insta::assert_snapshot!(stdout(&[
        "--dry-run",
        "draw",
        "shared/checkmark_front_8x8.image"
    ]));
}

#[test]
fn a_shared_prefix_resolves_to_stock_not_an_asset() {
    let payload = stdout(&["--dry-run", "draw", "shared/clock.image"]);
    assert!(payload.contains("\"stock_path\""), "got {payload}");
    assert!(!payload.contains("\"path\""), "got {payload}");
}

#[test]
fn a_bare_name_resolves_to_an_asset_not_stock() {
    let payload = stdout(&["--dry-run", "draw", "logo.png"]);
    assert!(payload.contains("\"path\": \"logo.png\""), "got {payload}");
    assert!(!payload.contains("stock_path"), "got {payload}");
}

#[test]
fn as_stock_forces_the_interpretation() {
    // `shared/` is the reserved namespace, but --as must be able to override
    // resolution for the pathological cases the spec anticipates.
    let output = busy()
        .args(["--dry-run", "draw", "logo.png", "--as", "stock"])
        .output()
        .expect("should run");
    // `logo.png` is not a valid stock path (`shared/[a-z0-9_.]+`), so forcing
    // it must fail loudly rather than silently drawing something else.
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn as_image_forces_a_shared_prefixed_name_to_resolve_as_an_asset() {
    // Mirror of `as_stock_forces_the_interpretation`: `shared/` is the
    // reserved namespace by default, but `--as image` must be able to force
    // asset resolution anyway. This is the branch Phase 4's template rule
    // will slot in next to.
    let payload = stdout(&["--dry-run", "draw", "shared/clock.image", "--as", "image"]);
    assert!(
        payload.contains("\"path\": \"shared/clock.image\""),
        "got {payload}"
    );
    assert!(!payload.contains("stock_path"), "got {payload}");
}

#[test]
fn the_element_id_defaults_to_image() {
    assert!(stdout(&["--dry-run", "draw", "logo.png"]).contains("\"id\": \"image\""));
}

#[test]
fn until_is_rejected_on_draw() {
    // `--until` is accepted by clap (it's flattened in from `DeliveryArgs`,
    // shared with `text`), but `draw` does not yet parse it — it must fail
    // loudly rather than silently doing nothing.
    let output = busy()
        .args([
            "--dry-run",
            "draw",
            "logo.png",
            "--until",
            "2026-01-01T00:00:00Z",
        ])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--until"), "got {stderr}");
    assert!(stderr.contains("--timeout"), "got {stderr}");
}

#[test]
fn opacity_outside_the_range_is_rejected() {
    let output = busy()
        .args(["--dry-run", "draw", "logo.png", "--opacity", "101"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn draw_with_no_name_and_no_file_is_an_error() {
    let output = busy()
        .args(["--dry-run", "draw"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
}
