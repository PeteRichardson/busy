# Phase 3 — Assets, Images, and `busy draw` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upload a local image of any common format to the bar and draw it — `busy asset upload ./logo.png` then `busy draw logo.png` — with the CLI doing the resizing the device refuses to do.

**Architecture:** A new `src/image.rs` decodes, fits, and re-encodes to PNG, and is the only module importing the `image` crate — the same containment rule `device.rs` already enforces for `busylib`. `src/cmd/asset.rs` and `src/cmd/draw.rs` join the existing `cmd/` module. Conversion happens at upload, so whatever reaches the device is guaranteed drawable and the error lands in the command holding the bytes.

**Tech Stack:** Rust 2024, `busylib` 0.0.11, `clap` 4, `image` 0.25 (png/jpeg/gif decode, PNG encode only), `tokio`, `wiremock` + `assert_cmd` + `insta` for tests.

**Source documents:**
- `docs/specs/2026-08-10-phase-3-assets-and-draw-design.md` — the authority for this phase
- `docs/specs/2026-08-09-busy-cli-ux-design.md` — the command surface (§3 `draw`, §5 assets, §8 hardware measurements)
- `docs/busy-cli-architecture.md` — device behaviour

## Global Constraints

- **Only `src/device.rs` may write `use busylib::…`.** Only `src/image.rs` may write `use image::…`. Everything else imports from those two.
- **Every CLI option is `Option<T>`. Never use clap's `default_value`.** Defaults live in exactly one place, `config::Defaults`.
- Exit codes: 0 success, 1 runtime failure, 2 usage error. `CliError::Usage` → 2; `Runtime` and `PriorityConflict` → 1.
- `busylib = "=0.0.11"` pinned, default features. The `reqwest`-only feature combination does not compile in 0.0.11.
- Short options follow `docs/plans/2026-08-10-short-option-names.md`. `-o` is free and takes `--opacity`.
- **Never modify a `.snap` file to make a test pass.** The existing golden snapshots are verified against real hardware; a change there means the wire payload moved.
- The device is at `http://10.0.4.20`, no token. Leave the display cleared after any real-device check.

### Verified against the real crates — do not re-derive

Checked by compiling and running before this plan was written:

- `ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?` sniffs the format from **content**, not the file extension.
- `DynamicImage::resize(w, h, FilterType::Lanczos3)` **fits inside** the box preserving aspect. A 200×100 source into a 72×16 target yields **32×16**, not 72×16. That is the desired "fit" semantics; do not "correct" it to fill the panel.
- `fitted.write_to(&mut Cursor::new(Vec::new()), ImageFormat::Png)` re-encodes.
- `client.assets().upload(app, file, bytes)`, `client.assets().delete(app)`, `client.storage().list(StoragePath)` all exist and compile with these signatures.
- `ImageElement::asset(path)?`, `ImageElement::stock(stock_path)?`, `.opacity(Opacity::new(u8)?)`, and `DisplayElementBuilder::image(ImageElement)` compile.
- An `ImageElement`'s `path`/`stock_path` serialize **flattened**, as siblings of `type`, because `ImageSource` is `#[serde(untagged)]`.

### Measured on hardware — the reason this phase exists

- The device **decodes PNG natively**; we never produce its `.image` format.
- It **crops oversized images silently and returns 200**. A 200×100 logo renders as its top-left corner with no error. Resizing is the load-bearing work.
- It **rejects JPEG and GIF at draw time with a 400**, though `assets/upload` accepts the bytes happily. That seam is why conversion belongs at upload.
- It handles **colour → greyscale for the back panel** itself.

---

## File Structure

```
src/
├── image.rs          # NEW — decode, fit, encode PNG. Only importer of the `image` crate.
├── cmd/
│   ├── asset.rs      # NEW — upload | list | delete
│   ├── draw.rs       # NEW — resolution + payload construction
│   ├── clear.rs      # unchanged
│   ├── text.rs       # unchanged
│   └── mod.rs        # gains `pub mod asset;` and `pub mod draw;`
├── device.rs         # gains upload/list_assets/delete_assets + image type re-exports
├── config.rs         # gains Defaults::panel(screen)
├── validate.rs       # its private FRONT/BACK constants move to config
└── cli.rs            # gains Draw and Asset subcommands
tests/
├── asset.rs          # NEW
└── draw.rs           # NEW
```

---

## Task 1: `src/image.rs` — decode, fit, encode

The only genuinely new logic in this phase, and it is pure: no I/O, no network, no device. That makes it the best-tested part.

**Files:**
- Create: `src/image.rs`
- Modify: `Cargo.toml`, `src/main.rs`, `src/config.rs`, `src/validate.rs`

**Interfaces:**
- Consumes: `crate::error::CliError`, `crate::device::Screen`.
- Produces:
  - `config::Defaults::panel(screen: Screen) -> (u32, u32)` — `(72, 16)` front, `(160, 80)` back.
  - `image::Prepared { pub png: Vec<u8>, pub original: (u32, u32), pub final_size: (u32, u32) }` with `pub fn was_resized(&self) -> bool`.
  - `image::prepare(bytes: &[u8], target: (u32, u32)) -> Result<Prepared, CliError>`.

- [ ] **Step 1: Add the dependency**

```bash
cargo add image --no-default-features --features png,jpeg,gif
```

Expect `image = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "gif"] }`. Decode those three formats, encode PNG only — that keeps the tree as small as the crate allows. **Do not enable default features**; they pull in a dozen codecs we never read.

- [ ] **Step 2: Move the panel dimensions into `config::Defaults`**

`src/validate.rs` currently owns these privately at lines 10-11:

```rust
const FRONT: (i16, i16) = (72, 16);
const BACK: (i16, i16) = (160, 80);
```

`image.rs` needs the same numbers, and duplicating them guarantees drift. Delete both, and add to the `impl Defaults` block in `src/config.rs`, directly below `position`:

```rust
    /// Pixel dimensions of a display. The front panel is 72x16 RGB; the back is
    /// 160x80 in 16 greys. `position` above is the centre of these.
    pub fn panel(screen: Screen) -> (u32, u32) {
        match screen {
            Screen::Front => (72, 16),
            Screen::Back => (160, 80),
        }
    }
```

Then in `src/validate.rs`, replace the constant lookup inside `bounds_warnings` with a call, casting once at the boundary since coordinates are `i16` and sizes are unsigned:

```rust
        let (width, height) = crate::config::Defaults::panel(screen);
        let (width, height) = (width as i16, height as i16);
```

`use crate::config;` may need adding at the top of `validate.rs`. There is no module cycle: `config` does not import `validate`.

- [ ] **Step 3: Run the existing tests to prove the refactor changed nothing**

Run: `cargo test`
Expected: PASS, 87 tests. The 12 `validate` tests exercise both panels, so they are the proof. If any fails, the cast or the mapping is wrong — fix it before continuing.

- [ ] **Step 4: Write the failing tests**

Create `src/image.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::prepare;
    use image::{DynamicImage, ImageFormat};
    use std::io::Cursor;

    /// A synthetic PNG of the given size. Building fixtures with the same crate
    /// we decode with keeps the test self-contained — no binary files in the repo.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(width, height)
            .write_to(&mut out, ImageFormat::Png)
            .expect("encoding a blank image should not fail");
        out.into_inner()
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(width, height)
            .write_to(&mut out, ImageFormat::Jpeg)
            .expect("encoding a blank image should not fail");
        out.into_inner()
    }

    const FRONT: (u32, u32) = (72, 16);
    const BACK: (u32, u32) = (160, 80);

    #[test]
    fn an_oversized_image_is_scaled_down_preserving_aspect() {
        // 200x100 is 2:1; the front panel is 4.5:1. Fitting inside it is
        // height-limited, so the result is 32x16 — NOT 72x16. Verified against
        // the image crate before this plan was written.
        let out = prepare(&png(200, 100), FRONT).expect("should prepare");
        assert_eq!(out.original, (200, 100));
        assert_eq!(out.final_size, (32, 16));
        assert!(out.was_resized());
    }

    #[test]
    fn a_small_image_is_never_enlarged() {
        // An 8x8 icon stays 8x8. Blowing it up to fill the panel would be a
        // silent quality loss the user never asked for.
        let out = prepare(&png(8, 8), FRONT).expect("should prepare");
        assert_eq!(out.final_size, (8, 8));
        assert!(!out.was_resized());
    }

    #[test]
    fn an_exactly_panel_sized_image_passes_through() {
        let out = prepare(&png(72, 16), FRONT).expect("should prepare");
        assert_eq!(out.final_size, (72, 16));
        assert!(!out.was_resized());
    }

    #[test]
    fn a_portrait_source_is_width_limited_on_the_back_panel() {
        // 50x200 into 160x80 is height-limited: 20x80.
        let out = prepare(&png(50, 200), BACK).expect("should prepare");
        assert_eq!(out.final_size, (20, 80));
    }

    #[test]
    fn the_output_is_always_png_whatever_went_in() {
        let out = prepare(&jpeg(40, 10), FRONT).expect("should prepare");
        assert_eq!(&out.png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic bytes");
        assert_eq!(out.final_size, (40, 10));
    }

    #[test]
    fn the_format_is_sniffed_from_content_not_a_filename() {
        // prepare() never sees a path, so a .png-named JPEG cannot fool it.
        assert!(prepare(&jpeg(8, 8), FRONT).is_ok());
    }

    #[test]
    fn every_accepted_input_format_decodes() {
        // The three formats the `image` features enable. Verified before this
        // plan was written that all three round-trip at these sizes.
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Gif] {
            let mut encoded = Cursor::new(Vec::new());
            DynamicImage::new_rgb8(40, 10)
                .write_to(&mut encoded, format)
                .expect("encoding should not fail");
            let out = prepare(&encoded.into_inner(), FRONT)
                .unwrap_or_else(|error| panic!("{format:?} should decode: {error}"));
            assert_eq!(out.final_size, (40, 10), "{format:?}");
            assert_eq!(&out.png[..8], b"\x89PNG\r\n\x1a\n", "{format:?} must become PNG");
        }
    }

    #[test]
    fn undecodable_bytes_name_the_formats_we_accept() {
        let error = prepare(b"this is not an image at all", FRONT)
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("PNG"), "got {error}");
        assert!(error.contains("JPEG"), "got {error}");
        assert!(error.contains("GIF"), "got {error}");
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test image`
Expected: FAIL — `prepare` is not defined.

- [ ] **Step 6: Write the implementation**

Prepend to `src/image.rs`:

```rust
//! Preparing a local image for the bar.
//!
//! The only module that imports the `image` crate, for the same reason
//! `device.rs` is the only one importing `busylib`: one file to fix when an
//! upstream layout moves.
//!
//! The device decodes PNG natively but **crops** anything larger than the
//! panel, silently, returning 200. Fitting the image here is what turns a
//! logo that renders as its top-left corner into one that renders whole.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};

use crate::error::CliError;

/// An image decoded, fitted, and re-encoded ready to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// PNG bytes. The device decodes PNG; nothing else needs to be produced.
    pub png: Vec<u8>,
    pub original: (u32, u32),
    pub final_size: (u32, u32),
}

impl Prepared {
    /// Whether fitting changed the dimensions. A resize the user cannot see is
    /// the problem this phase exists to fix, so callers warn on this.
    pub fn was_resized(&self) -> bool {
        self.original != self.final_size
    }
}

/// Decode `bytes`, scale down to fit inside `target` preserving aspect ratio,
/// and re-encode as PNG.
///
/// Never enlarges: a source already inside the target is re-encoded unchanged
/// in dimensions.
pub fn prepare(bytes: &[u8], target: (u32, u32)) -> Result<Prepared, CliError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| CliError::usage(format!("could not read the image: {error}")))?;

    let decoded = reader.decode().map_err(|error| {
        CliError::usage(format!(
            "could not decode the image: {error}\n\
             `busy` reads PNG, JPEG, and GIF, and uploads PNG (the only format the \
             bar decodes)."
        ))
    })?;

    let original = (decoded.width(), decoded.height());
    let (target_width, target_height) = target;

    let fitted: DynamicImage = if original.0 > target_width || original.1 > target_height {
        // `resize` fits inside the box preserving aspect — 200x100 into 72x16
        // gives 32x16, not 72x16. Fit, not fill: cropping is precisely the
        // device behaviour we are protecting against.
        decoded.resize(target_width, target_height, FilterType::Lanczos3)
    } else {
        decoded
    };
    let final_size = (fitted.width(), fitted.height());

    let mut png = Cursor::new(Vec::new());
    fitted
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| CliError::runtime(format!("could not encode as PNG: {error}")))?;

    Ok(Prepared {
        png: png.into_inner(),
        original,
        final_size,
    })
}
```

Add `mod image;` to `src/main.rs`, alphabetically — it sorts between `error` and `input`.

**Naming collision to expect:** the module is `crate::image` and the crate is `image`. Inside `src/image.rs`, `use image::…` resolves to the crate (Rust 2018+ path rules), so the file compiles as written. Elsewhere, refer to `crate::image::prepare`.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test image`
Expected: PASS, 8 tests.

Then `cargo test` (94 total), `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. All clean.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/image.rs src/main.rs src/config.rs src/validate.rs
git commit -m "feat: decode, fit, and re-encode images for the panel"
```

---

## Task 2: Device asset operations

**Files:**
- Modify: `src/device.rs`
- Test: `tests/asset.rs` (created here, extended by Tasks 3-5)

**Interfaces:**
- Consumes: `config::Settings`, `error::CliError`.
- Produces, on `Device`:
  - `pub async fn upload(&self, file: &str, bytes: Vec<u8>) -> Result<(), CliError>`
  - `pub async fn list_assets(&self) -> Result<Vec<StorageListElement>, CliError>`
  - `pub async fn delete_assets(&self) -> Result<(), CliError>`
- Also re-exported from `device`: `ImageElement`, `ImageSource`, `AssetName`, `AssetPath`, `StockPath`, `Opacity`, `StorageListElement`.

- [ ] **Step 1: Write the failing test**

Create `tests/asset.rs`:

```rust
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
```

`ok()` is unused in this file for now; Tasks 3-5 use it. If clippy objects to the unused import before then, import only what each task needs.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test asset`
Expected: FAIL — there is no `asset` subcommand yet, so clap exits 2.

- [ ] **Step 3: Extend `src/device.rs`**

Add to the re-export block at the top:

```rust
pub use busylib::model::assets::{
    Align, DisplayElement, DisplayElements, ElementKind, Font, ImageElement, ImageSource,
    Lifetime, Screen, TextElement,
};
pub use busylib::model::storage::StorageListElement;
pub use busylib::types::asset_name::AssetName;
pub use busylib::types::asset_path::AssetPath;
pub use busylib::types::opacity::Opacity;
pub use busylib::types::stock_path::StockPath;
```

Add to the `impl Device` block:

```rust
    /// Write one asset into this application's directory. Overwrites in place.
    pub async fn upload(&self, file: &str, bytes: Vec<u8>) -> Result<(), CliError> {
        self.client
            .assets()
            .upload(self.app.clone(), file, bytes)
            .await
            .map_err(|error| map_error(error, None))
    }

    /// This application's assets, newest listing from the device itself.
    ///
    /// App assets live at `/ext/user_assets/<application_name>/` — undocumented,
    /// learned from the text of a 400. `DELETE assets/upload` removes the
    /// directory rather than emptying it, so a 400 here means "no assets"
    /// rather than a failure.
    pub async fn list_assets(&self) -> Result<Vec<StorageListElement>, CliError> {
        let path = format!("/ext/user_assets/{}", self.app);
        let path = StoragePath::new(path)
            .map_err(|error| CliError::runtime(format!("invalid asset path: {error}")))?;

        match self.client.storage().list(path).await {
            Ok(entries) => Ok(entries),
            Err(error) if error.is_status(StatusCode::BAD_REQUEST) => Ok(Vec::new()),
            Err(error) => Err(map_error(error, None)),
        }
    }

    /// Delete every asset belonging to this application.
    ///
    /// All-or-nothing: the API offers no per-file delete. `storage/remove`
    /// returns 400 on a real asset path and the file survives — measured.
    pub async fn delete_assets(&self) -> Result<(), CliError> {
        self.client
            .assets()
            .delete(self.app.clone())
            .await
            .map_err(|error| map_error(error, None))
    }
```

Add `use busylib::types::storage_path::StoragePath;` to the imports. `StatusCode` is already imported for `map_error`.

- [ ] **Step 4: Run the tests to verify the failure moved**

Run: `cargo test --test asset`
Expected: still FAIL, but now because the `asset` subcommand does not exist rather than because `Device` lacks methods. Task 3 adds the command.

Confirm the crate still builds: `cargo build` succeeds, `cargo clippy --all-targets -- -D warnings` is clean. The new methods have no caller yet; if clippy reports them dead, add `#[expect(dead_code, reason = "…")]` and **delete it in Task 3** when the caller lands — `#[expect]` fails the build once it becomes untrue, which is the mechanism that forces the cleanup.

- [ ] **Step 5: Commit**

```bash
git add src/device.rs tests/asset.rs
git commit -m "feat: device asset upload, list, and delete-all"
```

---

## Task 3: `busy asset upload`

**Files:**
- Modify: `src/cli.rs`, `src/cmd/mod.rs`, `src/main.rs`
- Create: `src/cmd/asset.rs`
- Test: `tests/asset.rs`

**Interfaces:**
- Consumes: `image::prepare`, `config::Defaults::panel`, `Device::upload`, `Emitter`.
- Produces: `cli::{AssetCmd, AssetUploadArgs, AssetDeleteArgs}`; `cmd::asset::upload(args: &AssetUploadArgs, settings: &Settings, emitter: &Emitter, dry_run: bool) -> Result<(), CliError>`.

- [ ] **Step 1: Write the failing test**

Append to `tests/asset.rs`:

```rust
use std::io::Write as _;

/// Write a real PNG of the given size to a temp path and return it.
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
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
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

    let path = std::env::temp_dir().join(format!("busy-test-{name}"));
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
    assert!(stderr.contains("200x100"), "should name the original, got {stderr}");
    assert!(stderr.contains("32x16"), "should name the result, got {stderr}");
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test asset`
Expected: FAIL — no `asset` subcommand.

- [ ] **Step 3: Add the CLI surface**

In `src/cli.rs`, extend the `Command` enum:

```rust
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Draw a line of text
    Text(Box<TextArgs>),
    /// Draw an uploaded asset, a device built-in, or a raw payload
    Draw(Box<DrawArgs>),
    /// Manage this application's uploaded assets
    #[command(subcommand)]
    Asset(AssetCmd),
    /// Remove everything this application has drawn
    Clear,
}
```

`DrawArgs` arrives in Task 6; add a placeholder now so the enum compiles, and fill it in there:

```rust
#[derive(Args, Debug, Clone, Default)]
pub struct DrawArgs {
    /// Asset name, or a `shared/…` device built-in
    pub name: Option<String>,
}
```

Add the asset commands:

```rust
#[derive(Subcommand, Debug)]
pub enum AssetCmd {
    /// Convert, fit, and upload a local image
    Upload(AssetUploadArgs),
    /// List this application's assets, read from the device
    List,
    /// Delete ALL of this application's assets
    Delete(AssetDeleteArgs),
}

#[derive(Args, Debug, Clone)]
pub struct AssetUploadArgs {
    /// Local image file. PNG, JPEG, or GIF; always stored as PNG.
    pub path: PathBuf,

    /// Panel to fit the image for. This is the *fit target*, not where the
    /// image is drawn — repeat `--screen` on `busy draw` to render it there.
    #[arg(short, long, value_enum)]
    pub screen: Option<ScreenArg>,
}

#[derive(Args, Debug, Clone)]
pub struct AssetDeleteArgs {
    /// Skip the confirmation prompt
    #[arg(short, long)]
    pub yes: bool,
}
```

Add `use std::path::PathBuf;` at the top of `cli.rs`.

- [ ] **Step 4: Write `src/cmd/asset.rs`**

```rust
//! `busy asset` — upload, list, and delete this application's assets.

use crate::cli::AssetUploadArgs;
use crate::config::{self, Settings};
use crate::device::{AssetName, Device};
use crate::error::CliError;
use crate::image;
use crate::output::Emitter;

/// Convert a local image to a panel-sized PNG and upload it.
///
/// Conversion happens here rather than at draw time because `assets/upload` is
/// a dumb byte write: it accepts a JPEG happily and the failure only surfaces
/// later, from a different command, as a device error about an `/ext` path.
pub async fn upload(
    args: &AssetUploadArgs,
    settings: &Settings,
    emitter: &Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    let bytes = std::fs::read(&args.path).map_err(|error| {
        CliError::usage(format!("could not read {}: {error}", args.path.display()))
    })?;

    let screen = args
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    let target = config::Defaults::panel(screen);

    let prepared = image::prepare(&bytes, target)?;

    let stem = args
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            CliError::usage(format!("{} has no usable file name", args.path.display()))
        })?;
    let name = format!("{stem}.png");
    let name = AssetName::new(name.clone()).map_err(|error| {
        CliError::usage(format!(
            "`{name}` is not a usable asset name: {error}. Rename the file to use only \
             letters, digits, dot, underscore, or hyphen."
        ))
    })?;

    if prepared.was_resized() {
        emitter.warn(&format!(
            "resized {}x{} to {}x{} to fit the {} panel; the bar crops anything larger \
             without saying so",
            prepared.original.0,
            prepared.original.1,
            prepared.final_size.0,
            prepared.final_size.1,
            match screen {
                crate::device::Screen::Front => "front",
                crate::device::Screen::Back => "back",
            }
        ));
    }

    let original_name = args.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if original_name != name.as_str() {
        emitter.warn(&format!(
            "stored as `{name}`: the bar only decodes PNG, so `{original_name}` was \
             re-encoded"
        ));
    }

    if dry_run {
        return emitter.success(
            &format!(
                "would upload {} bytes as `{name}` ({}x{})",
                prepared.png.len(),
                prepared.final_size.0,
                prepared.final_size.1
            ),
            None,
        );
    }

    let device = Device::connect(settings)?;
    device.upload(name.as_str(), prepared.png).await?;

    emitter.success(&format!("uploaded `{name}`"), None)
}
```

- [ ] **Step 5: Wire it into `src/main.rs`**

Add `Asset` to the `match`:

```rust
        Command::Asset(asset) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            match asset {
                cli::AssetCmd::Upload(args) => {
                    cmd::asset::upload(args, &settings, emitter, cli.global.dry_run).await
                }
                cli::AssetCmd::List => Err(CliError::runtime("`busy asset list` arrives in Task 4")),
                cli::AssetCmd::Delete(_) => {
                    Err(CliError::runtime("`busy asset delete` arrives in Task 5"))
                }
            }
        }
        Command::Draw(_) => Err(CliError::runtime("`busy draw` arrives in Task 6")),
```

Add `pub mod asset;` to `src/cmd/mod.rs`.

Delete any `#[expect(dead_code)]` Task 2 put on `Device::upload` — it now has a caller, and `#[expect]` will fail the build if you leave it.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test asset`
Expected: the four upload tests PASS; the two listing tests still FAIL (Task 4).

Then `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`, both clean.

- [ ] **Step 7: Commit**

```bash
git add src/cli.rs src/cmd/asset.rs src/cmd/mod.rs src/main.rs tests/asset.rs
git commit -m "feat: busy asset upload with fit-to-panel conversion"
```

---

## Task 4: `busy asset list`

**Files:**
- Modify: `src/cmd/asset.rs`, `src/main.rs`
- Test: `tests/asset.rs` (the two tests written in Task 2)

**Interfaces:**
- Consumes: `Device::list_assets`, `StorageListElement`.
- Produces: `cmd::asset::list(settings: &Settings, emitter: &Emitter, dry_run: bool) -> Result<(), CliError>`.

- [ ] **Step 1: Run the existing failing tests**

Run: `cargo test --test asset listing` and `cargo test --test asset no_assets`
Expected: FAIL — `busy asset list` returns the Task 4 placeholder error.

- [ ] **Step 2: Implement**

Append to `src/cmd/asset.rs`:

```rust
/// List this application's assets, read from the device rather than from any
/// local record — there is no local record, deliberately.
pub async fn list(
    settings: &Settings,
    emitter: &Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    if dry_run {
        return emitter.success(
            &format!("would list assets for `{}`", settings.app),
            None,
        );
    }

    let device = Device::connect(settings)?;
    let entries = device.list_assets().await?;

    let mut files: Vec<(&str, u64)> = entries
        .iter()
        .filter(|entry| !entry.is_dir())
        .map(|entry| (entry.name(), entry.size().unwrap_or(0)))
        .collect();
    files.sort_by_key(|(name, _)| *name);

    if files.is_empty() {
        return emitter.success(&format!("no assets for `{}`", settings.app), None);
    }

    let mut report = String::new();
    for (name, size) in &files {
        report.push_str(&format!("{name}\t{size}\n"));
    }
    report.push_str(&format!("{} asset(s)", files.len()));

    emitter.success(&report, None)
}
```

In `src/main.rs`, replace the `List` arm:

```rust
                cli::AssetCmd::List => cmd::asset::list(&settings, emitter, cli.global.dry_run).await,
```

Delete Task 2's `#[expect(dead_code)]` on `Device::list_assets`.

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --test asset`
Expected: six of seven PASS; only the delete test (Task 5) still fails.

- [ ] **Step 4: Commit**

```bash
git add src/cmd/asset.rs src/main.rs
git commit -m "feat: busy asset list, reading the device directly"
```

---

## Task 5: `busy asset delete`

All-or-nothing, so the confirmation is the feature.

**Files:**
- Modify: `src/cmd/asset.rs`, `src/main.rs`
- Test: `tests/asset.rs`

**Interfaces:**
- Consumes: `Device::{list_assets, delete_assets}`, `AssetDeleteArgs`.
- Produces: `cmd::asset::delete(args: &AssetDeleteArgs, settings: &Settings, emitter: &Emitter, dry_run: bool) -> Result<(), CliError>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/asset.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test asset delete`
Expected: FAIL — `busy asset delete` returns the Task 5 placeholder error.

- [ ] **Step 3: Implement**

Append to `src/cmd/asset.rs`:

```rust
use std::io::IsTerminal as _;

/// Delete every asset belonging to this application.
///
/// The API has no per-file delete — `storage/remove` returns 400 on a real
/// asset path and the file survives — so this is all-or-nothing, and the file
/// list is printed first to make the blast radius concrete.
pub async fn delete(
    args: &AssetDeleteArgs,
    settings: &Settings,
    emitter: &Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    let device = Device::connect(settings)?;
    let entries = device.list_assets().await?;
    let names: Vec<&str> = entries
        .iter()
        .filter(|entry| !entry.is_dir())
        .map(|entry| entry.name())
        .collect();

    if names.is_empty() {
        return emitter.success(&format!("no assets for `{}`", settings.app), None);
    }

    let summary = format!(
        "this deletes ALL {} asset(s) for `{}`: {}",
        names.len(),
        settings.app,
        names.join(", ")
    );

    if dry_run {
        return emitter.success(&format!("would delete: {summary}"), None);
    }

    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(CliError::usage(format!(
                "{summary}\nRefusing to delete without confirmation. Re-run with --yes."
            )));
        }
        emitter.warn_always(&summary);
        eprint!("Delete them? [y/N] ");
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|error| CliError::usage(format!("could not read confirmation: {error}")))?;
        if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes") {
            return emitter.success("cancelled", None);
        }
    } else {
        emitter.warn_always(&summary);
    }

    device.delete_assets().await?;
    emitter.success(&format!("deleted {} asset(s)", names.len()), None)
}
```

Add `use crate::cli::AssetDeleteArgs;` to the imports at the top of the file.

In `src/main.rs`:

```rust
                cli::AssetCmd::Delete(args) => {
                    cmd::asset::delete(args, &settings, emitter, cli.global.dry_run).await
                }
```

Delete Task 2's `#[expect(dead_code)]` on `Device::delete_assets`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test asset`
Expected: PASS, 10 tests. Then `cargo test` (whole suite), clippy, and fmt — all clean.

- [ ] **Step 5: Commit**

```bash
git add src/cmd/asset.rs src/main.rs tests/asset.rs
git commit -m "feat: busy asset delete, with the blast radius shown first"
```

---

## Task 6: `busy draw` for assets and stock paths

**Files:**
- Modify: `src/cli.rs`, `src/cmd/mod.rs`, `src/main.rs`
- Create: `src/cmd/draw.rs`
- Test: `tests/draw.rs`

**Interfaces:**
- Consumes: `config::{Settings, resolve_align, screen_from_arg, parse_priority, Defaults}`, `device::{AssetPath, StockPath, ImageElement, Opacity, DisplayElement, DisplayElements}`.
- Produces:
  - `cli::DrawArgs` (filled in), `cli::AsArg { Image, Stock }`
  - `cmd::draw::Resolved { Asset(AssetPath), Stock(StockPath) }`
  - `cmd::draw::resolve(args: &DrawArgs) -> Result<Resolved, CliError>`
  - `cmd::draw::build_payload(args: &DrawArgs, settings: &Settings, file: &FileConfig, resolved: &Resolved) -> Result<DisplayElements, CliError>`

- [ ] **Step 1: Write the failing tests**

Create `tests/draw.rs`:

```rust
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
        "--dry-run", "draw", "logo.png", "--opacity", "50"
    ]));
}

#[test]
fn golden_payload_for_a_stock_path() {
    insta::assert_snapshot!(stdout(&[
        "--dry-run", "draw", "shared/checkmark_front_8x8.image"
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
fn the_element_id_defaults_to_image() {
    assert!(stdout(&["--dry-run", "draw", "logo.png"]).contains("\"id\": \"image\""));
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
    let output = busy().args(["--dry-run", "draw"]).output().expect("should run");
    assert_eq!(output.status.code(), Some(2));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test draw`
Expected: FAIL — `busy draw` returns the Task 6 placeholder error.

- [ ] **Step 3: Complete `DrawArgs` in `src/cli.rs`**

Replace the Task 3 placeholder:

```rust
#[derive(Args, Debug, Clone, Default)]
pub struct DrawArgs {
    /// Asset name, or a `shared/…` device built-in
    pub name: Option<String>,

    /// Draw a raw DisplayElements payload from a file instead of a named thing
    #[arg(long, conflicts_with = "name")]
    pub file: Option<PathBuf>,

    /// Force how the name is interpreted, for pathological cases
    #[arg(long = "as", value_enum)]
    pub as_kind: Option<AsArg>,

    /// Opacity, 0-100
    #[arg(short = 'o', long)]
    pub opacity: Option<u8>,

    #[command(flatten)]
    pub placement: PlacementArgs,

    #[command(flatten)]
    pub delivery: DeliveryArgs,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum AsArg {
    Image,
    Stock,
}
```

`as` is a Rust keyword, hence the field name `as_kind` with an explicit `long = "as"`.

- [ ] **Step 4: Write `src/cmd/draw.rs`**

```rust
//! `busy draw` — put a named thing on the bar.
//!
//! The unifying idea is that `draw` takes a name which expands to
//! `DisplayElements`. In this phase a name expands to a single `ImageElement`;
//! Phase 4 inserts template lookup between the stock and asset rules, so keep
//! `resolve` shaped for that insertion rather than restructuring it later.

use crate::cli::{AsArg, DrawArgs};
use crate::config::{self, FileConfig, Settings};
use crate::device::{
    AssetPath, DisplayElement, DisplayElements, ImageElement, Opacity, StockPath,
};
use crate::error::CliError;

/// What a `draw` name turned out to mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Asset(AssetPath),
    Stock(StockPath),
}

/// Resolve a name to a source.
///
/// 1. `shared/…` is the spec's reserved namespace for device built-ins.
/// 2. *(a local template directory — Phase 4, absent here.)*
/// 3. anything else is an asset in this application's directory.
pub fn resolve(args: &DrawArgs) -> Result<Resolved, CliError> {
    let name = args.name.as_deref().ok_or_else(|| {
        CliError::usage("`busy draw` needs a name or --file; see `busy draw --help`")
    })?;

    let as_stock = match args.as_kind {
        Some(AsArg::Stock) => true,
        Some(AsArg::Image) => false,
        None => name.starts_with("shared/"),
    };

    if as_stock {
        let stock = StockPath::new(name).map_err(|error| {
            CliError::usage(format!(
                "`{name}` is not a valid stock path: {error}. Device built-ins look like \
                 `shared/checkmark_front_8x8.image`."
            ))
        })?;
        return Ok(Resolved::Stock(stock));
    }

    let asset = AssetPath::new(name).map_err(|error| {
        CliError::usage(format!("`{name}` is not a valid asset name: {error}"))
    })?;
    Ok(Resolved::Asset(asset))
}

/// Build the wire payload. Pure: no I/O, no network, so `--dry-run` and the
/// real send are guaranteed to produce identical bytes.
pub fn build_payload(
    args: &DrawArgs,
    settings: &Settings,
    file: &FileConfig,
    resolved: &Resolved,
) -> Result<DisplayElements, CliError> {
    let mut element = match resolved {
        Resolved::Asset(path) => ImageElement::asset(path.clone()),
        Resolved::Stock(path) => ImageElement::stock(path.clone()),
    }
    .map_err(|error| CliError::usage(error.to_string()))?;

    if let Some(percent) = args.opacity {
        let opacity = Opacity::new(percent).map_err(|error| {
            CliError::usage(format!("invalid --opacity: {error}"))
        })?;
        element = element.opacity(opacity);
    }

    let screen = args
        .placement
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    let (default_x, default_y) = config::Defaults::position(screen);

    let id = args
        .delivery
        .id
        .clone()
        .unwrap_or_else(|| "image".to_owned());
    let id = crate::device::ElementId::new(id)
        .map_err(|error| CliError::usage(format!("invalid --id: {error}")))?;

    let mut builder = DisplayElement::builder(id)
        .map_err(|error| CliError::usage(error.to_string()))?
        .at(
            args.placement.x.unwrap_or(default_x),
            args.placement.y.unwrap_or(default_y),
        )
        .screen(screen)
        .align(config::resolve_align(args.placement.align, file));

    if let Some(seconds) = args.delivery.timeout {
        builder = builder.timeout_secs(seconds);
    }

    let priority_value = match &args.delivery.priority {
        Some(input) => config::parse_priority(input)?,
        None => settings.priority,
    };
    let priority = crate::device::Priority::new(priority_value)
        .map_err(|error| CliError::usage(format!("invalid --priority: {error}")))?;

    let app = crate::device::AppName::new(settings.app.clone())
        .map_err(|error| CliError::usage(format!("invalid --app: {error}")))?;

    let mut payload = DisplayElements::new(app)
        .map_err(|error| CliError::usage(error.to_string()))?
        .priority(priority)
        .element(builder.image(element));

    if let Some(input) = &args.delivery.led {
        payload = payload.led_notification_color(crate::color::parse(input)?);
    }

    Ok(payload)
}
```

`--until` is deliberately not handled here: `text` owns the RFC 3339 parsing, and duplicating it would be the kind of copy this project has been careful to avoid. If `--until` on `draw` is wanted, extract `parse_until` from `cmd/text.rs` into a shared home first — that is a separate change, not this task's.

- [ ] **Step 5: Wire it into `src/main.rs`**

```rust
        Command::Draw(args) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            let resolved = cmd::draw::resolve(args)?;
            let payload = cmd::draw::build_payload(args, &settings, &file, &resolved)?;

            for warning in validate::bounds_warnings(&payload) {
                emitter.warn(&warning);
            }

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            let device = device::Device::connect(&settings)?;
            if !args.delivery.keep {
                device.clear().await?;
            }
            device.draw(&payload).await?;

            emitter.success("drawn", Some(&payload))
        }
```

Add `pub mod draw;` to `src/cmd/mod.rs`.

- [ ] **Step 6: Run the tests and accept the snapshots**

Run: `cargo test --test draw`
Expected: the two `insta` snapshots are new; the rest pass.

Generate and **inspect** them — `cargo insta review` is interactive, so use `INSTA_UPDATE=always cargo test --test draw`, then read the `.snap` files before accepting. The asset payload must be exactly this, verified against `busylib` 0.0.11's own serialization before this plan was written:

```json
{
  "application_name": "busy",
  "priority": 95,
  "elements": [
    {
      "id": "image",
      "x": 36,
      "y": 8,
      "display": "front",
      "align": "center",
      "type": "image",
      "path": "logo.png",
      "opacity": 50
    }
  ]
}
```

Note `path` sits as a **sibling** of `type`, not nested, because `ImageSource` is `#[serde(untagged)]` and flattened. The stock payload is the same shape with `"stock_path"` in place of `"path"` and no `opacity`.

**If a generated snapshot disagrees with this, investigate — do not accept it.**

- [ ] **Step 7: Verify the whole suite**

Run: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. All clean.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs src/cmd/draw.rs src/cmd/mod.rs src/main.rs tests/draw.rs tests/snapshots
git commit -m "feat: busy draw for uploaded assets and device built-ins"
```

---

## Task 7: `busy draw --file`

**Files:**
- Modify: `src/cmd/draw.rs`, `src/main.rs`
- Test: `tests/draw.rs`

**Interfaces:**
- Consumes: `DrawArgs::file`.
- Produces: `cmd::draw::load_file(path: &std::path::Path) -> Result<DisplayElements, CliError>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/draw.rs`:

```rust
#[test]
fn a_raw_payload_file_is_drawn_verbatim() {
    let path = std::env::temp_dir().join("busy-test-payload.json");
    std::fs::write(
        &path,
        r#"{
            "application_name": "busy",
            "priority": 95,
            "elements": [
                {"id": "a", "type": "text", "text": "from a file", "font": "small"}
            ]
        }"#,
    )
    .expect("write payload");

    let output = busy()
        .args(["--dry-run", "draw", "--file"])
        .arg(&path)
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("from a file"), "got {stdout}");
}

#[test]
fn a_malformed_payload_file_names_the_path_and_the_problem() {
    let path = std::env::temp_dir().join("busy-test-bad.json");
    std::fs::write(&path, "{ not json at all").expect("write payload");

    let output = busy()
        .args(["--dry-run", "draw", "--file"])
        .arg(&path)
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("busy-test-bad.json"), "got {stderr}");
}

#[test]
fn file_and_a_name_are_mutually_exclusive() {
    let output = busy()
        .args(["--dry-run", "draw", "logo.png", "--file", "/tmp/x.json"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn id_is_an_error_with_file_because_ids_come_from_the_payload() {
    // A payload file names its own elements. Silently ignoring --id would let
    // a user believe they had renamed something they had not.
    let path = std::env::temp_dir().join("busy-test-id.json");
    std::fs::write(
        &path,
        r#"{"application_name":"busy","elements":[{"id":"a","type":"text","text":"x","font":"small"}]}"#,
    )
    .expect("write payload");

    let output = busy()
        .args(["--dry-run", "draw", "--id", "mine", "--file"])
        .arg(&path)
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--id"), "got {stderr}");
    assert!(stderr.contains("--file"), "got {stderr}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test draw file`
Expected: the first two FAIL (`--file` is parsed but ignored, so `resolve` errors with "needs a name or --file"); `file_and_a_name_are_mutually_exclusive` already passes from the clap `conflicts_with`.

- [ ] **Step 3: Implement**

Append to `src/cmd/draw.rs`:

```rust
/// Load a raw `DisplayElements` payload from a file.
///
/// The template file format in Phase 4 deserializes into the same type, which
/// is what makes animation, countdown, and rectangle elements reachable without
/// this project modelling them. This is the same door, opened early.
pub fn load_file(path: &std::path::Path) -> Result<DisplayElements, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::usage(format!("could not read {}: {error}", path.display())))?;

    serde_json::from_str(&text).map_err(|error| {
        CliError::usage(format!(
            "{} is not a valid display payload: {error}",
            path.display()
        ))
    })
}
```

In `src/main.rs`, branch before resolving a name:

```rust
        Command::Draw(args) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;

            let payload = match &args.file {
                Some(path) => {
                    // A payload file names its own elements. Silently ignoring
                    // --id would let the user believe they had renamed
                    // something they had not.
                    if args.delivery.id.is_some() {
                        return Err(CliError::usage(
                            "--id cannot be used with --file: element ids come from the \
                             payload. Edit the file's `id` fields instead.",
                        ));
                    }
                    cmd::draw::load_file(path)?
                }
                None => {
                    let resolved = cmd::draw::resolve(args)?;
                    cmd::draw::build_payload(args, &settings, &file, &resolved)?
                }
            };
            // …unchanged from Task 6 below this point…
```

This is the `--id` row of the command-surface spec's §3.4 table: `text` defaults to `message`, an image draw to `image`, and `--file` (like a template, later) is an error.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test draw`
Expected: PASS, 12 tests. Then the whole suite, clippy, and fmt.

- [ ] **Step 5: Commit**

```bash
git add src/cmd/draw.rs src/main.rs tests/draw.rs
git commit -m "feat: busy draw --file for raw display payloads"
```

---

## Task 8: Probe assertions, README, and the release pass

**Files:**
- Modify: `scripts/probe-device.sh`, `README.md`
- Test: the whole suite, plus the real device

**Interfaces:**
- Consumes: everything.
- Produces: no new API.

- [ ] **Step 1: Add the image assertions to the probe script**

`scripts/probe-device.sh` asserts device behaviours the OpenAPI document does not specify, so a firmware change is caught rather than silently altering the CLI's assumptions. Two of this phase's measurements belong there. Insert before the cleanup step, renumbering it as before:

```sh
say "10. oversized images — expect a 200 and a CROPPED render, not a scaled one"
# A 16x16 red square; drawn at 8x8 the device would scale, at 16x16 it crops.
printf '%s' \
  'iVBORw0KGgoAAAANSUhEUgAAABAAAAAQAQAAAAA3iMLMAAAAEUlEQVR4nGP4z0AswKqSCAAA//8DAAoAAv8Ex4CqAAAAAElFTkSuQmCC' \
  | base64 -d > "$PROBE.big"
curl -s -H "$AUTH" -H 'Content-Type: application/octet-stream' --data-binary "@$PROBE.big" \
  "$BAR/assets/upload?application_name=$APP&file=big.png" ; echo
curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
  -d "{\"application_name\":\"$APP\",\"priority\":95,\"elements\":[{\"id\":\"b\",\"type\":\"image\",\"path\":\"big.png\",\"x\":0,\"y\":0,\"align\":\"top_left\"}]}" ; echo

say "11. JPEG — expect upload 200 and draw 400 (the device decodes PNG only)"
sips -s format jpeg "$PROBE" --out "$PROBE.jpg" >/dev/null 2>&1 || echo "  (sips unavailable, skipping)"
if [ -f "$PROBE.jpg" ]; then
  curl -s -H "$AUTH" -H 'Content-Type: application/octet-stream' --data-binary "@$PROBE.jpg" \
    "$BAR/assets/upload?application_name=$APP&file=probe.jpg" ; echo
  curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
    -d "{\"application_name\":\"$APP\",\"priority\":95,\"elements\":[{\"id\":\"j\",\"type\":\"image\",\"path\":\"probe.jpg\"}]}" ; echo
fi
```

Add `"$PROBE.big" "$PROBE.jpg"` to the `trap … rm -f` line so the temp files are cleaned up.

- [ ] **Step 2: Update the README**

Add the asset and draw commands to the example block, and a note under the existing caveats. Keep it honest — the two facts that will bite a user are the crop and the panel-specific fit:

```markdown
busy asset upload ./logo.png       # fit for the front panel, stored as logo.png
busy draw logo.png                 # draw it
busy draw shared/checkmark_front_8x8.image
busy asset list
```

And in the notes:

```markdown
- **Images are fitted, not cropped.** The bar decodes PNG and silently crops
  anything larger than the panel, so `busy asset upload` scales the image down
  to fit and tells you when it did. JPEG and GIF are converted to PNG on upload
  (the bar decodes PNG only) and stored under a `.png` name.
- **`--screen` on `asset upload` is the fit target, not the destination.** An
  image fitted for the back panel still needs `busy draw --screen back` to be
  drawn there; drawn on the front, the bar will crop it.
- **Assets are all-or-nothing to delete.** The API has no per-file delete, so
  `busy asset delete` removes every asset for the app and asks first.
```

- [ ] **Step 3: Run the full gate**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all green, roughly 105 tests.

- [ ] **Step 4: Verify against the real bar**

The device is at `http://10.0.4.20`, no token. This is the acceptance test for the whole phase — an oversized image must arrive whole rather than cropped:

```bash
# Build a 200x100 source, upload it, and draw it.
cargo run -- asset upload /tmp/box_200x100.png    # expect: "resized 200x100 to 32x16"
cargo run -- asset list                            # expect: box_200x100.png with a size
cargo run -- draw box_200x100.png

# Read the frame back and confirm the whole image is on the panel.
curl -s "http://10.0.4.20/api/screen?display=0" -o /tmp/f.b64
python3 -c "
import base64
raw=base64.b64decode(open('/tmp/f.b64').read()); W,H=72,16
for y in range(H): print(''.join('#' if any(raw[(y*W+x)*3:(y*W+x)*3+3]) else '.' for x in range(W)))
"

cargo run -- asset delete --yes
cargo run -- clear
```

The pixel view is the proof: a **fitted** 32×16 image shows the whole figure in the left third of the panel. A **cropped** one would show the top-left corner stretched across the full width — which is what the old behaviour looked like. Paste both the resize message and the pixel view into the report.

Leave the display cleared.

- [ ] **Step 5: Commit**

```bash
git add scripts/probe-device.sh README.md
git commit -m "docs: probe assertions and README for assets and draw"
```

---

## Definition of done

- `busy asset upload ./logo.jpg` converts, fits, reports both the resize and the rename, and uploads a drawable PNG.
- `busy asset list` reads the device and prints "no assets" rather than an error for an app with none.
- `busy asset delete` shows what it will destroy and refuses without `--yes` when there is no tty.
- `busy draw logo.png`, `busy draw shared/….image`, and `busy draw --file payload.json` all work, and `--dry-run` contacts nothing.
- An oversized image drawn on the real bar appears whole, not cropped — verified by reading the frame back.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` are green.

## Execution notes, learned from Phases 1–2

Carried forward because they measurably improved the second half of that run, and are
not recorded anywhere else.

**The plan is the likeliest source of defects, not the implementers.** Phases 1–2 ran
nine real bugs from the plan against two from implementers. Transcription was reliable;
specification was not. Two were caught by a pre-flight scan of the plan before dispatching
anything — do that scan. Look especially for a constraint the plan then violates itself,
and for code the plan mandates that the review rubric would call a defect.

**Have reviewers write their report to a file and return a short verdict.** A reviewer's
full report stays resident in the controller's context for the rest of the session, and
its value drops to near zero the moment it has been acted on. Inline reports cost ~2.5k
tokens each; a file plus a ten-line verdict costs ~300 for the same information.

**Verify by running the binary, not by reading a green suite.** Every one of the most
serious findings in Phases 1–2 came from execution, not from tests: a warning that
claimed "11px does not fit 72px", a `--json` mode that emitted invalid JSON whenever a
warning fired, a success line printed for a draw that never happened. All three had
passing tests. For this phase, the equivalent is reading a frame back off the device —
a fitted image and a cropped one both exit 0.

**Sonnet is the floor for implementers.** Haiku took 33 minutes and 30 tool calls on a
task Sonnet did in 88 seconds. The cheap tier costs more in wall-clock and context than
it saves.

**Treat an Important finding as a fix round even if the summary says "Approved".** One
Phase 1–2 review did exactly that, and the finding — a test that checked two requests
happened but not their order — would have let a `POST`-then-`DELETE` regression wipe the
element it had just drawn and still exit 0.

**Expect API errors mid-task.** Three subagents died to connection errors, one twice on
the same task. Recovery was cheap every time because the work was uncommitted and the
ledger recorded the position; check the working tree before re-dispatching, since a dead
agent often got further than its last message suggests.

## Deferred, and where it is recorded

Templates and `--var`, the inert-flag check, `--id`-is-an-error-for-templates: Phase 4. The replace-by-default flicker fix and the fourth invisibility mode: `docs/specs/2026-08-09-busy-cli-ux-design.md` §9. A draw-time warning when an asset's dimensions suit the other panel: §10 of this phase's spec — it needs a `storage/read` round trip per draw, so it waits until the annoyance is real.
