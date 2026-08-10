mod common;

use common::{busy_at, ok};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn listing_reads_the_apps_asset_directory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .and(query_param("path", "/ext/user_assets/busy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "list": [
                {"type": "file", "name": "logo.png", "size": 451},
                {"type": "file", "name": "icon.png", "size": 73}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "list"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("logo.png"), "got {stdout}");
    assert!(stdout.contains("451"), "should show the size, got {stdout}");
}

#[tokio::test]
async fn an_app_with_no_assets_is_not_an_error() {
    // Delete-all removes the directory rather than emptying it, so a 400 here
    // means "no assets", not a failure. Measured on hardware.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "Bad Request"
        })))
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "list"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "a missing directory must not fail");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no assets"),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

use std::io::Write as _;

/// Write a real PNG of the given size to a temp path and return it.
///
/// Each fixture gets its own temp subdirectory so the basename can be kept
/// exactly as passed — `png_file("big.png", ...)` produces a file literally
/// named `big.png`, not `busy-test-big.png`, which is what `upload` derives
/// the asset name from and what the mocks below assert on.
fn png_file(name: &str, width: u32, height: u32) -> std::path::PathBuf {
    // A minimal, valid RGB PNG built by hand so the test needs no image crate
    // dependency of its own. Solid black, no filtering.
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&[kind, data].concat());
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }
    fn crc32(bytes: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *entry = c;
        }
        let mut c = 0xFFFF_FFFFu32;
        for b in bytes {
            c = table[((c ^ u32::from(*b)) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }

    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB

    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0u8); // filter: none
        raw.extend(std::iter::repeat_n(0u8, (width * 3) as usize));
    }
    // Store-only zlib stream: header, then deflate "stored" blocks.
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = u8::from((i + 1) * 65535 >= raw.len());
        z.push(last);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let mut adler = (1u32, 0u32);
    for b in &raw {
        adler.0 = (adler.0 + u32::from(*b)) % 65521;
        adler.1 = (adler.1 + adler.0) % 65521;
    }
    z.extend_from_slice(&((adler.1 << 16) | adler.0).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &z));
    png.extend(chunk(b"IEND", b""));

    let dir = std::env::temp_dir().join(format!("busy-test-{}", name.replace('.', "-")));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).expect("temp file");
    f.write_all(&png).expect("write png");
    path
}

#[tokio::test]
async fn uploading_fits_the_image_and_reports_the_resize() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/assets/upload"))
        .and(query_param("application_name", "busy"))
        .and(query_param("file", "big.png"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let source = png_file("big.png", 200, 100);
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("200x100"),
        "should name the original, got {stderr}"
    );
    assert!(
        stderr.contains("32x16"),
        "should name the result, got {stderr}"
    );
}

#[tokio::test]
async fn a_non_png_source_is_renamed_and_the_rename_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/assets/upload"))
        .and(query_param("file", "logo.png"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    // The bytes are PNG; only the extension differs. The stored name must
    // follow the format we upload, not the name we were given.
    let source = png_file("logo.jpg", 8, 8);
    let output = busy_at(&server)
        .args(["asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("logo.png"),
        "the rename must be reported"
    );
}

#[tokio::test]
async fn upload_honours_dry_run() {
    let server = MockServer::start().await;
    // No mocks: any request would 404 and fail the command.
    let source = png_file("dry.png", 8, 8);
    let output = busy_at(&server)
        .args(["--dry-run", "asset", "upload"])
        .arg(&source)
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[test]
fn a_missing_source_file_is_a_usage_error() {
    let output = common::busy()
        .args(["asset", "upload", "/nonexistent/nope.png"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("nope.png"));
}

#[tokio::test]
async fn delete_with_yes_lists_first_then_deletes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "list": [{"type": "file", "name": "logo.png", "size": 451}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/assets/upload"))
        .and(query_param("application_name", "busy"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "delete", "--yes"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The blast radius must be shown even when confirmation is skipped.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("logo.png"), "got {combined}");
}

#[tokio::test]
async fn delete_refuses_without_yes_when_not_a_terminal() {
    // The test harness gives the child a piped stdin, so there is no tty to
    // prompt on. Refusing is the only safe answer: prompting into the void
    // would hang, and deleting silently would be destructive.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "list": [{"type": "file", "name": "logo.png", "size": 451}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/assets/upload"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "delete"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--yes"),
        "the error must name the escape hatch"
    );
}

#[tokio::test]
async fn deleting_when_there_is_nothing_to_delete_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "Bad Request"
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/assets/upload"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["asset", "delete", "--yes"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("no assets"));
}
