//! Lenient colour parsing.
//!
//! `busylib::types::color::Color::parse` requires exactly `#RRGGBBAA`. Users
//! type `red`, `0xff0000`, and `#f00`, so the CLI accepts those and constructs
//! the validated type itself.

use crate::device::Color;

/// Named colours the CLI understands, as `#RRGGBB`.
/// Used by the parse function.
#[allow(dead_code)]
const NAMES: &[(&str, u32)] = &[
    ("red", 0xFF0000),
    ("green", 0x00FF00),
    ("blue", 0x0000FF),
    ("white", 0xFFFFFF),
    ("black", 0x000000),
    ("yellow", 0xFFFF00),
    ("orange", 0xFFA500),
    ("cyan", 0x00FFFF),
    ("magenta", 0xFF00FF),
];

/// Parse a colour string into a validated Color.
/// This function is used by the CLI command handlers and is tested separately.
#[allow(dead_code)]
pub fn parse(input: &str) -> Result<Color, String> {
    let trimmed = input.trim();

    let lower = trimmed.to_ascii_lowercase();
    if let Some((_, rgb)) = NAMES.iter().find(|(name, _)| *name == lower) {
        let [_, red, green, blue] = rgb.to_be_bytes();
        return Ok(Color::rgb(red, green, blue));
    }

    let hex = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("0x"))
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(input));
    }

    let nibble = |index: usize| -> u8 {
        u8::from_str_radix(&hex[index..index + 1], 16).expect("checked hex digit")
    };
    let byte = |index: usize| -> u8 {
        u8::from_str_radix(&hex[index..index + 2], 16).expect("checked hex digit")
    };

    match hex.len() {
        3 => Ok(Color::rgb(nibble(0) * 17, nibble(1) * 17, nibble(2) * 17)),
        6 => Ok(Color::rgb(byte(0), byte(2), byte(4))),
        8 => Ok(Color::rgba(byte(0), byte(2), byte(4), byte(6))),
        _ => Err(invalid(input)),
    }
}

/// Format an error message for invalid colour input.
/// Used by the parse function and its tests.
#[allow(dead_code)]
fn invalid(input: &str) -> String {
    format!(
        "invalid colour `{input}`: expected #RRGGBBAA, #RRGGBB, #RGB, a 0x-prefixed \
         or bare hex value, or one of red, green, blue, white, black, yellow, \
         orange, cyan, magenta"
    )
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn hex(input: &str) -> String {
        parse(input).expect("should parse").to_string()
    }

    #[test]
    fn accepted_forms_all_reach_rrggbbaa() {
        assert_eq!(hex("#FF0000FF"), "#FF0000FF");
        assert_eq!(hex("#ff0000ff"), "#FF0000FF");
        assert_eq!(hex("0xFF0000FF"), "#FF0000FF");
        assert_eq!(hex("FF0000FF"), "#FF0000FF");
        assert_eq!(hex("#FF0000"), "#FF0000FF", "6 digits gain an opaque alpha");
        assert_eq!(hex("0xFF0000"), "#FF0000FF");
        assert_eq!(hex("FF0000"), "#FF0000FF");
        assert_eq!(
            hex("#F00"),
            "#FF0000FF",
            "3-digit shorthand doubles each nibble"
        );
        assert_eq!(hex("#abc"), "#AABBCCFF");
        assert_eq!(hex("red"), "#FF0000FF");
        assert_eq!(hex("GREEN"), "#00FF00FF", "names are case-insensitive");
        assert_eq!(hex("blue"), "#0000FFFF");
        assert_eq!(hex("white"), "#FFFFFFFF");
        assert_eq!(hex("black"), "#000000FF");
        assert_eq!(hex("yellow"), "#FFFF00FF");
        assert_eq!(hex("orange"), "#FFA500FF");
        assert_eq!(hex("cyan"), "#00FFFFFF");
        assert_eq!(hex("magenta"), "#FF00FFFF");
    }

    #[test]
    fn alpha_is_preserved_when_given() {
        assert_eq!(hex("#FF000080"), "#FF000080");
    }

    #[test]
    fn rejected_forms_explain_themselves() {
        for bad in ["", "#", "#FF", "#FFFFF", "nope", "#GGGGGG", "0x"] {
            let error = parse(bad).expect_err("should be rejected");
            assert!(
                error.contains(bad) || bad.is_empty(),
                "error for {bad:?} should quote the input, got {error:?}"
            );
        }
    }
}
