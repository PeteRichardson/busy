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
// Not yet constructed outside tests — the asset commands that call `prepare`
// land in a later task of this phase. `cfg_attr(not(test), ...)` keeps the
// expectation accurate under both `cargo test` (where the unit tests below
// use it) and `cargo clippy --all-targets` (where the non-test build does
// not); once real callers exist, drop this and let dead-code analysis run
// normally.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired up by the asset-upload task later in this phase"
    )
)]
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "wired up by the asset-upload task later in this phase"
        )
    )]
    pub fn was_resized(&self) -> bool {
        self.original != self.final_size
    }
}

/// Decode `bytes`, scale down to fit inside `target` preserving aspect ratio,
/// and re-encode as PNG.
///
/// Never enlarges: a source already inside the target is re-encoded unchanged
/// in dimensions.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired up by the asset-upload task later in this phase"
    )
)]
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
            assert_eq!(
                &out.png[..8],
                b"\x89PNG\r\n\x1a\n",
                "{format:?} must become PNG"
            );
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
