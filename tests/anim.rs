//! `.anim` animations: uploading them intact, and drawing them as animations.
//!
//! The container is the BUSY Bar's own (`bicycle0`); `src/anim.rs` documents
//! the layout. These tests build one by hand rather than checking in a binary
//! fixture, so a reader can see what every field means.

mod common;

use std::io::Write as _;

use common::{busy_at, ok};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer};

/// Build a valid `.anim`: one raw frame, one mandatory `default` section.
fn anim_bytes(width: u8, height: u8, fps: u8) -> Vec<u8> {
    let pixels = vec![0u8; usize::from(width) * usize::from(height) * 3];

    let mut frame = vec![0u8, 1]; // raw encoding, held for one display frame
    frame.extend((pixels.len() as u16).to_le_bytes());
    frame.extend(&pixels);

    let mut section = Vec::new();
    section.extend(0u32.to_le_bytes()); // first display frame
    section.extend(0u32.to_le_bytes()); // last display frame
    section.extend(57u32.to_le_bytes()); // 36-byte header + this 21-byte chunk
    section.push(1); // duration override
    section.extend(b"default\0");

    let mut header = b"bicycle0".to_vec();
    header.extend([0, width, height, 0]); // flags, size, rgb888
    header.push(fps);
    header.extend((pixels.len() as u16).to_le_bytes()); // longest encoded frame
    header.push(0); // unused
    header.extend((section.len() as u32).to_le_bytes());
    header.extend((frame.len() as u32).to_le_bytes());
    header.extend(1u32.to_le_bytes()); // sections
    header.extend(1u32.to_le_bytes()); // stored frames
    header.extend(1u32.to_le_bytes()); // displayed frames

    [header, section, frame].concat()
}

fn anim_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("busy-anim-{}", name.replace('.', "-")));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    std::fs::File::create(&path)
        .expect("temp file")
        .write_all(bytes)
        .expect("write anim");
    path
}

/// The whole point of the upload branch: an animation is not an image and must
/// arrive at the device unaltered. Re-encoding it as PNG, which is what every
/// other upload does, would destroy it.
#[tokio::test]
async fn an_animation_uploads_byte_for_byte() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/assets/upload"))
        .and(query_param("file", "spin.anim"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let bytes = anim_bytes(8, 8, 30);
    let source = anim_file("spin.anim", &bytes);
    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let sent = &server.received_requests().await.expect("requests")[0];
    assert_eq!(sent.body, bytes, "the animation must not be re-encoded");
}

#[tokio::test]
async fn the_upload_summary_describes_the_animation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/assets/upload"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let source = anim_file("described.anim", &anim_bytes(8, 8, 30));
    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("8x8"), "got {stdout}");
    assert!(stdout.contains("30 fps"), "got {stdout}");
    assert!(stdout.contains("rgb888"), "got {stdout}");
}

/// The device answers 200 to a malformed animation and then shows solid
/// magenta, so if this check does not fire here nothing else will.
#[tokio::test]
async fn a_truncated_animation_is_refused_before_any_request() {
    let server = MockServer::start().await;
    // No mocks are mounted: reaching the network at all would 404 and fail.

    let bytes = anim_bytes(8, 8, 30);
    let source = anim_file("cut.anim", &bytes[..bytes.len() - 8]);
    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2), "a bad file is a usage error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a usable .anim"), "got {stderr}");
    assert!(
        server
            .received_requests()
            .await
            .expect("requests")
            .is_empty(),
        "nothing should have been sent"
    );
}

/// Named `.anim` but not one. The image decoder's "could not determine the
/// format" would point at the wrong problem entirely.
#[tokio::test]
async fn a_file_named_anim_that_is_not_one_says_so() {
    let server = MockServer::start().await;
    let source = anim_file("liar.anim", b"\x89PNG\r\n\x1a\nnot an animation");

    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bicycle0"), "got {stderr}");
}

/// An oversized animation is legitimate — it is how a sprite sheet is done —
/// so this must be advice, not a failure.
#[tokio::test]
async fn an_oversized_animation_uploads_with_a_hint_about_panning() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/assets/upload"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let source = anim_file("sheet.anim", &anim_bytes(216, 16, 10));
    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    assert!(output.status.success(), "an oversized animation is valid");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("216x16"), "got {stderr}");
    assert!(
        stderr.contains("-x"),
        "should point at panning, got {stderr}"
    );
}

/// `--dry-run` never reaches the network, so these need no mock device.
fn dry_run_payload(args: &[&str]) -> serde_json::Value {
    let output = common::busy()
        .args(["--dry-run", "draw"])
        .args(args)
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("dry run prints JSON")
}

#[tokio::test]
async fn a_dot_anim_name_draws_an_animation_element() {
    let payload = dry_run_payload(&["horse.anim"]);
    let element = &payload["elements"][0];

    assert_eq!(element["type"], "animation");
    assert_eq!(element["path"], "horse.anim");
    // Omitted rather than guessed: the device owns the default.
    assert!(element.get("loop").is_none(), "got {element}");
    assert!(element.get("section").is_none(), "got {element}");
}

#[tokio::test]
async fn a_png_name_still_draws_an_image_element() {
    let payload = dry_run_payload(&["logo.png"]);
    assert_eq!(payload["elements"][0]["type"], "image");
}

#[tokio::test]
async fn a_stock_animation_resolves_to_a_stock_path() {
    let payload = dry_run_payload(&["shared/spinner_front_8x8.anim"]);
    let element = &payload["elements"][0];

    assert_eq!(element["type"], "animation");
    assert_eq!(element["stock_path"], "shared/spinner_front_8x8.anim");
}

#[tokio::test]
async fn loop_and_section_reach_the_payload() {
    let payload = dry_run_payload(&["horse.anim", "--loop", "--section", "gallop"]);
    let element = &payload["elements"][0];

    assert_eq!(element["loop"], true);
    assert_eq!(element["section"], "gallop");
}

/// Distinct from the image default so `--keep` composes the two rather than
/// having the second draw evict the first.
#[tokio::test]
async fn the_element_id_defaults_to_animation() {
    assert_eq!(
        dry_run_payload(&["horse.anim"])["elements"][0]["id"],
        "animation"
    );
    assert_eq!(dry_run_payload(&["logo.png"])["elements"][0]["id"], "image");
}

#[tokio::test]
async fn opacity_still_applies_to_an_animation() {
    let payload = dry_run_payload(&["horse.anim", "--opacity", "40"]);
    assert_eq!(payload["elements"][0]["opacity"], 40);
}

/// A negative anchor is how the panel-sized window is moved across an
/// oversized animation, so the off-display warning must not fire for one.
/// Verified on hardware: a 216x16 animation at x=-144 shows its last 72
/// columns exactly.
#[tokio::test]
async fn panning_an_animation_off_the_left_edge_is_not_warned_about() {
    let output = common::busy()
        .args([
            "--dry-run",
            "draw",
            "sheet.anim",
            "-x",
            "-144",
            "-y",
            "0",
            "-a",
            "top_left",
        ])
        .output()
        .expect("should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "should be silent, got {stderr}");
}

#[tokio::test]
async fn the_same_offset_on_an_image_is_still_warned_about() {
    let output = common::busy()
        .args(["--dry-run", "draw", "logo.png", "-x", "-144"])
        .output()
        .expect("should run");

    assert!(
        String::from_utf8_lossy(&output.stderr).contains("render nothing"),
        "an image really does vanish there"
    );
}

#[tokio::test]
async fn loop_is_rejected_on_an_image_draw() {
    let output = common::busy()
        .args(["--dry-run", "draw", "logo.png", "--loop"])
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--loop"), "got {stderr}");
    assert!(stderr.contains("`.anim`"), "got {stderr}");
}

#[tokio::test]
async fn section_is_rejected_on_a_payload_file() {
    let dir = std::env::temp_dir().join("busy-anim-payload");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("payload.json");
    std::fs::write(
        &file,
        r##"{"application_name":"busy","elements":[{"id":"a","type":"text","text":"hi","font":"large","color":"#ffffffff"}]}"##,
    )
    .expect("write payload");

    let output = common::busy()
        .args(["--dry-run", "draw", "--file"])
        .arg(&file)
        .args(["--section", "whatever"])
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--section"), "got {stderr}");
    assert!(
        stderr.contains("which element"),
        "should explain the ambiguity, got {stderr}"
    );
}
