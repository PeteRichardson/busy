//! Transliteration of common Unicode into the printable ASCII the device accepts.
//!
//! `Text` is `^[\x20-\x7E]+$` because the fonts are bitmap ASCII. A build
//! notification that fails because of a smart quote is a terrible experience,
//! so the CLI fixes what it can and warns once about what it changed.

/// The result of sanitizing, and whether anything was altered.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no caller until Task 5 wires to_ascii into the send path"
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub text: String,
    pub changed: bool,
}

/// Transliterate `input` into printable ASCII, dropping what can't be mapped.
/// Not yet called outside tests; Task 5 wires it into the send path.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no caller until Task 5 wires this into the send path"
    )
)]
pub fn to_ascii(input: &str) -> Sanitized {
    let mut text = String::with_capacity(input.len());
    let mut changed = false;

    for character in input.chars() {
        match character {
            // Already acceptable.
            '\x20'..='\x7e' => text.push(character),

            // Quotes.
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => {
                text.push('\'');
                changed = true;
            }
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => {
                text.push('"');
                changed = true;
            }

            // Dashes and minus.
            '\u{2010}'..='\u{2015}' | '\u{2212}' => {
                text.push('-');
                changed = true;
            }

            // Whitespace of every kind collapses to a plain space. The display
            // is one line, so a newline is more useful as a space than as
            // nothing.
            '\u{00a0}' | '\u{2007}' | '\u{202f}' | '\u{2009}' | '\t' | '\n' | '\r' => {
                text.push(' ');
                changed = true;
            }

            // Miscellaneous.
            '\u{2026}' => {
                text.push_str("...");
                changed = true;
            }
            '\u{2022}' => {
                text.push('*');
                changed = true;
            }

            // Anything else — emoji, other control characters — is dropped.
            _ => changed = true,
        }
    }

    Sanitized { text, changed }
}

#[cfg(test)]
mod tests {
    use super::to_ascii;

    #[test]
    fn plain_ascii_is_untouched() {
        let result = to_ascii("Build failed: 3 tests");
        assert_eq!(result.text, "Build failed: 3 tests");
        assert!(!result.changed);
    }

    #[test]
    fn common_punctuation_transliterates() {
        let cases = [
            ("don\u{2019}t", "don't"),
            ("\u{2018}quoted\u{2019}", "'quoted'"),
            ("\u{201c}quoted\u{201d}", "\"quoted\""),
            ("an \u{2013} en dash", "an - en dash"),
            ("an \u{2014} em dash", "an - em dash"),
            ("wait\u{2026}", "wait..."),
            ("non\u{00a0}breaking", "non breaking"),
            ("bullet \u{2022} point", "bullet * point"),
        ];
        for (input, expected) in cases {
            let result = to_ascii(input);
            assert_eq!(result.text, expected, "input {input:?}");
            assert!(result.changed, "input {input:?} should report a change");
        }
    }

    #[test]
    fn unmapped_non_ascii_is_dropped() {
        let result = to_ascii("done \u{1f389}");
        assert_eq!(result.text, "done ");
        assert!(result.changed);
    }

    #[test]
    fn line_endings_and_tabs_become_spaces() {
        // The bar draws a single line, so whitespace is more useful collapsed
        // to a space than dropped: "line break" beats "linebreak".
        let result = to_ascii("line\nbreak\ttab");
        assert_eq!(result.text, "line break tab");
        assert!(result.changed);
    }

    #[test]
    fn other_control_characters_are_dropped() {
        let result = to_ascii("bell\u{0007}here");
        assert_eq!(result.text, "bellhere");
        assert!(result.changed);
    }

    #[test]
    fn a_message_can_sanitize_to_nothing() {
        let result = to_ascii("\u{1f389}\u{1f389}");
        assert_eq!(result.text, "");
        assert!(result.changed);
    }
}
