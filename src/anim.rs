//! Reading the header of a BUSY Bar `.anim` file.
//!
//! `.anim` is the bar's native animation container, signature `bicycle0`. The
//! CLI never builds one — that is a generator's job — but it has to recognise
//! one, because an animation travels a different path from an image at every
//! step: `asset upload` must not re-encode it as PNG, and `draw` must emit an
//! animation element rather than an image element.
//!
//! The validation here mirrors the firmware's own loader
//! (`lib/anim_file/components/anim_file_load.c`) and exists because the device
//! gives no feedback: measured 2026-08-12 on API 25.0.0, uploading a truncated
//! `.anim` answers 200, drawing it answers 200, and the panel then shows solid
//! magenta. Nothing in the HTTP conversation says the file was rejected. So a
//! file that cannot play is worth catching here, before it is uploaded.
//!
//! Only the header is parsed. Frame data is the device's business.

use crate::error::CliError;

/// The container's magic. "Busybar Image Container speciallY Crafted for file
/// Length Eradication, ver. 0", per the firmware.
pub const SIGNATURE: &[u8; 8] = b"bicycle0";

/// Bytes before the sections chunk.
const HEADER_LEN: usize = 36;

/// Fixed part of a section descriptor: start, end, frame_offs, and the
/// duration override, before its NUL-terminated name.
const SECTION_FIXED_LEN: usize = 13;

/// The section that must exist, spanning the whole animation.
const DEFAULT_SECTION: &[u8] = b"default";

/// How pixels are stored. Named as the firmware names them, including the
/// lie: `Rgb888` puts blue first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    Rgb888,
    Gray4,
    Argb8888,
}

impl ColorFormat {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(ColorFormat::Rgb888),
            1 => Some(ColorFormat::Gray4),
            2 => Some(ColorFormat::Argb8888),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ColorFormat::Rgb888 => "rgb888",
            ColorFormat::Gray4 => "gray4",
            ColorFormat::Argb8888 => "argb8888",
        }
    }
}

/// What the header of a `.anim` says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anim {
    pub width: u8,
    pub height: u8,
    pub color_format: ColorFormat,
    pub fps: u8,
    /// Frames as played, counting a held frame once per display frame.
    pub display_frames: u32,
    /// Frames as stored. Lower than `display_frames` when frames repeat.
    pub file_frames: u32,
    /// Every section name, `default` first.
    pub sections: Vec<String>,
}

impl Anim {
    /// Roughly how long one pass takes. `None` when the header claims 0 fps,
    /// which `parse` rejects — kept total so callers need no second guard.
    pub fn duration_secs(&self) -> Option<f64> {
        (self.fps > 0).then(|| f64::from(self.display_frames) / f64::from(self.fps))
    }

    /// Section names other than the mandatory `default`.
    pub fn named_sections(&self) -> &[String] {
        self.sections.get(1..).unwrap_or(&[])
    }
}

/// Whether these bytes open with the `.anim` signature.
///
/// Cheap enough to call on every upload, which is the point: the file's
/// extension is a hint, and this is the fact.
pub fn is_anim(bytes: &[u8]) -> bool {
    bytes.starts_with(SIGNATURE)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn malformed(detail: impl std::fmt::Display) -> CliError {
    CliError::usage(format!(
        "not a usable .anim file: {detail}\n\
         The bar accepts a malformed animation without complaint and then \
         displays solid magenta, so `busy` checks it here instead."
    ))
}

/// Parse and validate the header, applying the same rules the firmware's
/// loader applies.
///
/// Every check here is one the device makes silently. Nothing is inferred
/// beyond them: frame data is not decoded, so a file can pass this and still
/// contain nonsense pixels.
pub fn parse(bytes: &[u8]) -> Result<Anim, CliError> {
    if !is_anim(bytes) {
        return Err(malformed(format!(
            "expected it to start with `{}`",
            String::from_utf8_lossy(SIGNATURE)
        )));
    }
    if bytes.len() < HEADER_LEN {
        return Err(malformed(format!(
            "the header needs {HEADER_LEN} bytes and the file has {}",
            bytes.len()
        )));
    }

    let flags = bytes[8];
    if flags != 0 {
        return Err(malformed(format!(
            "flags must be 0 and are {flags}; the firmware defines no flag yet \
             and rejects every other value"
        )));
    }

    let width = bytes[9];
    let height = bytes[10];
    let color_format = ColorFormat::from_byte(bytes[11])
        .ok_or_else(|| malformed(format!("colour format {} is not one of 0-2", bytes[11])))?;
    let fps = bytes[12];
    if fps == 0 {
        return Err(malformed("0 fps: it would never advance a frame"));
    }

    let sections_len = u32_at(bytes, 16) as usize;
    let frames_len = u32_at(bytes, 20) as usize;
    let section_count = u32_at(bytes, 24);
    let file_frames = u32_at(bytes, 28);
    let display_frames = u32_at(bytes, 32);

    if section_count == 0 {
        return Err(malformed("no sections; `default` is mandatory"));
    }
    if display_frames == 0 {
        return Err(malformed("no frames"));
    }

    // The check that catches truncation, which is otherwise invisible until
    // the panel turns magenta.
    let expected = HEADER_LEN
        .checked_add(sections_len)
        .and_then(|total| total.checked_add(frames_len))
        .ok_or_else(|| malformed("the chunk lengths in the header overflow"))?;
    if bytes.len() != expected {
        return Err(malformed(format!(
            "the header describes {expected} bytes and the file is {}",
            bytes.len()
        )));
    }

    let sections = parse_sections(bytes, sections_len, section_count, display_frames)?;

    Ok(Anim {
        width,
        height,
        color_format,
        fps,
        display_frames,
        file_frames,
        sections,
    })
}

fn parse_sections(
    bytes: &[u8],
    sections_len: usize,
    section_count: u32,
    display_frames: u32,
) -> Result<Vec<String>, CliError> {
    let chunk = bytes
        .get(HEADER_LEN..HEADER_LEN + sections_len)
        .ok_or_else(|| malformed("the sections chunk runs past the end of the file"))?;

    if chunk.last() != Some(&0) {
        return Err(malformed(
            "the sections chunk does not end with a NUL; the last section name is unterminated",
        ));
    }

    let mut names = Vec::new();
    let mut pos = 0;
    while pos + SECTION_FIXED_LEN < chunk.len() {
        let name_bytes = chunk[pos + SECTION_FIXED_LEN..]
            .iter()
            .take_while(|byte| **byte != 0)
            .copied()
            .collect::<Vec<_>>();

        if names.is_empty() {
            validate_default_section(chunk, &name_bytes, sections_len, display_frames)?;
        }

        pos += SECTION_FIXED_LEN + name_bytes.len() + 1;
        names.push(String::from_utf8_lossy(&name_bytes).into_owned());
    }

    if names.len() as u32 != section_count {
        return Err(malformed(format!(
            "the header claims {section_count} section(s) and the chunk holds {}",
            names.len()
        )));
    }

    Ok(names)
}

/// Section 0 carries precomputed start info the firmware trusts rather than
/// derives, so a wrong value is not caught by any later check — it just plays
/// from the wrong offset.
fn validate_default_section(
    chunk: &[u8],
    name: &[u8],
    sections_len: usize,
    display_frames: u32,
) -> Result<(), CliError> {
    if name != DEFAULT_SECTION {
        return Err(malformed(format!(
            "the first section is `{}` and must be `default`",
            String::from_utf8_lossy(name)
        )));
    }

    let start = u32_at(chunk, 0);
    let end = u32_at(chunk, 4);
    let frame_offs = u32_at(chunk, 8);

    if start != 0 || end != display_frames - 1 {
        return Err(malformed(format!(
            "`default` covers frames {start}..{end} and must cover 0..{}",
            display_frames - 1
        )));
    }

    let expected = (HEADER_LEN + sections_len) as u32;
    if frame_offs != expected {
        return Err(malformed(format!(
            "`default` points at byte {frame_offs} for its first frame; the frames \
             chunk starts at {expected}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest valid file: one 2x2 frame, stored raw, one section.
    fn minimal() -> Vec<u8> {
        let frame_data = vec![0u8; 2 * 2 * 3];
        let frame = [
            vec![0u8, 1], // raw, duration 1
            (frame_data.len() as u16).to_le_bytes().to_vec(),
            frame_data,
        ]
        .concat();

        let mut section = vec![];
        section.extend(0u32.to_le_bytes()); // start
        section.extend(0u32.to_le_bytes()); // end
        section.extend((HEADER_LEN as u32 + 21).to_le_bytes()); // frame_offs
        section.push(1); // duration override
        section.extend(DEFAULT_SECTION);
        section.push(0);
        assert_eq!(section.len(), 21);

        let mut header = vec![];
        header.extend(SIGNATURE);
        header.extend([0, 2, 2, 0]); // flags, width, height, colour format
        header.extend([30]); // fps
        header.extend((frame_data_len() as u16).to_le_bytes()); // max encoded
        header.push(0); // unused
        header.extend((section.len() as u32).to_le_bytes());
        header.extend((frame.len() as u32).to_le_bytes());
        header.extend(1u32.to_le_bytes()); // sections
        header.extend(1u32.to_le_bytes()); // file frames
        header.extend(1u32.to_le_bytes()); // display frames
        assert_eq!(header.len(), HEADER_LEN);

        [header, section, frame].concat()
    }

    fn frame_data_len() -> usize {
        2 * 2 * 3
    }

    #[test]
    fn a_minimal_file_parses() {
        let anim = parse(&minimal()).expect("the fixture should be valid");

        assert_eq!(anim.width, 2);
        assert_eq!(anim.height, 2);
        assert_eq!(anim.color_format, ColorFormat::Rgb888);
        assert_eq!(anim.fps, 30);
        assert_eq!(anim.display_frames, 1);
        assert_eq!(anim.sections, vec!["default".to_owned()]);
        assert!(anim.named_sections().is_empty());
    }

    #[test]
    fn the_signature_is_what_identifies_an_anim() {
        assert!(is_anim(&minimal()));
        assert!(!is_anim(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_anim(b""));
    }

    #[test]
    fn a_png_is_rejected_by_name() {
        let error = parse(b"\x89PNG\r\n\x1a\nrest of a png").unwrap_err();
        assert!(error.to_string().contains("bicycle0"), "{error}");
    }

    /// The check that matters most: the device accepts a truncated file and
    /// shows magenta, so this is the only place it can be caught.
    #[test]
    fn truncation_is_caught_by_the_length_equation() {
        let full = minimal();
        let error = parse(&full[..full.len() - 4]).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("the header describes"), "{message}");
        assert!(message.contains("magenta"), "{message}");
    }

    #[test]
    fn trailing_junk_is_caught_too() {
        let mut padded = minimal();
        padded.push(0);
        assert!(parse(&padded).is_err());
    }

    #[test]
    fn a_short_file_cannot_even_hold_a_header() {
        let error = parse(&minimal()[..20]).unwrap_err();
        assert!(error.to_string().contains("36 bytes"), "{error}");
    }

    #[test]
    fn unknown_flags_are_rejected_as_the_firmware_rejects_them() {
        let mut file = minimal();
        file[8] = 1;
        assert!(parse(&file).unwrap_err().to_string().contains("flags"));
    }

    #[test]
    fn an_unknown_colour_format_is_rejected() {
        let mut file = minimal();
        file[11] = 3;
        assert!(
            parse(&file)
                .unwrap_err()
                .to_string()
                .contains("colour format")
        );
    }

    #[test]
    fn zero_fps_is_rejected() {
        let mut file = minimal();
        file[12] = 0;
        assert!(parse(&file).unwrap_err().to_string().contains("0 fps"));
    }

    #[test]
    fn the_first_section_must_be_named_default() {
        let mut file = minimal();
        // "default" lives at 36 + 13; rename it in place, same length.
        file[HEADER_LEN + SECTION_FIXED_LEN..HEADER_LEN + SECTION_FIXED_LEN + 7]
            .copy_from_slice(b"custom!");

        let error = parse(&file).unwrap_err();
        assert!(error.to_string().contains("must be `default`"), "{error}");
    }

    #[test]
    fn the_default_section_must_span_every_frame() {
        let mut file = minimal();
        file[HEADER_LEN + 4..HEADER_LEN + 8].copy_from_slice(&7u32.to_le_bytes());

        let error = parse(&file).unwrap_err();
        assert!(error.to_string().contains("must cover 0..0"), "{error}");
    }

    #[test]
    fn a_wrong_precomputed_frame_offset_is_rejected() {
        let mut file = minimal();
        file[HEADER_LEN + 8..HEADER_LEN + 12].copy_from_slice(&99u32.to_le_bytes());

        let error = parse(&file).unwrap_err();
        assert!(error.to_string().contains("points at byte 99"), "{error}");
    }

    #[test]
    fn a_section_count_that_disagrees_with_the_chunk_is_rejected() {
        let mut file = minimal();
        file[24..28].copy_from_slice(&2u32.to_le_bytes());

        let error = parse(&file).unwrap_err();
        assert!(error.to_string().contains("claims 2 section"), "{error}");
    }

    #[test]
    fn duration_is_frames_over_fps() {
        let mut anim = parse(&minimal()).unwrap();
        anim.display_frames = 60;
        anim.fps = 30;

        assert_eq!(anim.duration_secs(), Some(2.0));
    }
}
