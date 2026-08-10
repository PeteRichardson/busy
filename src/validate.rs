//! Local checks that run before a request is sent.
//!
//! An element outside the display bounds renders nothing and the device reports
//! no error, so these are warnings the CLI raises itself. They never block a
//! send — the user may know something the CLI does not.

use crate::device::{Align, DisplayElements, ElementKind, Font, Screen};

/// Front display: 72x16 RGB. Back display: 160x80 in 16 greys.
const FRONT: (i16, i16) = (72, 16);
const BACK: (i16, i16) = (160, 80);

/// Approximate rendered width per character, in pixels, measured on real
/// hardware (API 25.0.0) by drawing known strings and reading back the frame
/// via `GET /screen`. `large`: "Hello, World!" (13 chars) inked 70px wide.
/// `small`: the same string inked 41px.
///
/// These are estimates for a warning, never for layout: the fonts are not
/// strictly monospaced, so treat the result as "probably too wide", not as a
/// measurement.
fn px_per_char(font: Font) -> f32 {
    match font {
        Font::Tiny => 2.6,
        Font::Small => 3.2,
        Font::Condensed => 4.0,
        Font::Normal | Font::Global => 4.6,
        Font::Bold => 5.0,
        Font::Large => 5.4,
        Font::ExtraLarge => 7.2,
    }
}

/// Where an anchor places the element relative to the anchor point, per axis:
/// -1 means the element extends in the negative direction, 0 means it is
/// centred on the point, +1 means it extends in the positive direction.
fn anchor_direction(align: Align) -> (i8, i8) {
    match align {
        Align::TopLeft => (1, 1),
        Align::TopMid => (0, 1),
        Align::TopRight => (-1, 1),
        Align::MidLeft => (1, 0),
        Align::Center => (0, 0),
        Align::MidRight => (-1, 0),
        Align::BottomLeft => (1, -1),
        Align::BottomMid => (0, -1),
        Align::BottomRight => (-1, -1),
    }
}

pub fn bounds_warnings(payload: &DisplayElements) -> Vec<String> {
    let mut warnings = Vec::new();

    for element in &payload.elements {
        let screen = element.display.unwrap_or(Screen::Front);
        let (width, height) = match screen {
            Screen::Front => FRONT,
            Screen::Back => BACK,
        };
        let screen_name = match screen {
            Screen::Front => "front",
            Screen::Back => "back",
        };

        let x = element.x.unwrap_or(0);
        let y = element.y.unwrap_or(0);
        let id = &element.id;

        // 1. The anchor point itself is off the display.
        if x < 0 || x >= width || y < 0 || y >= height {
            warnings.push(format!(
                "element `{id}` is anchored at ({x}, {y}), outside the {screen_name} \
                 display's {width}x{height} bounds; it will render nothing"
            ));
            continue;
        }

        // 2. The anchor is in bounds, but the direction the element extends
        //    puts it entirely off the display. Measured on hardware: at (0,0),
        //    five of the nine align values render a completely blank screen
        //    while the device still returns 200 OK. The anchor-point check
        //    above cannot see this, because (0,0) is in bounds.
        //
        //    This only runs when align is known: a payload built from a
        //    template file may legitimately omit it, and guessing a
        //    direction to warn about would be worse than saying nothing.
        //    When it does fire, the element is already known to render
        //    nothing, so the width check below is skipped for it — a second,
        //    unrelated-sounding warning about the same invisible element
        //    would only be noise.
        let mut already_invisible = false;
        if let Some(align) = element.align {
            let (dx, dy) = anchor_direction(align);

            if dx < 0 && x == 0 {
                warnings.push(format!(
                    "element `{id}` uses align `{align:?}`, which anchors its right edge at \
                     x={x}, so it extends off the left of the {screen_name} display and will \
                     render nothing"
                ));
                already_invisible = true;
            }
            if dy < 0 && y == 0 {
                warnings.push(format!(
                    "element `{id}` uses align `{align:?}`, which anchors its bottom edge at \
                     y={y}, so it extends off the top of the {screen_name} display and will \
                     render nothing"
                ));
                already_invisible = true;
            }
        }
        if already_invisible {
            continue;
        }

        // 3. Text that is probably too wide for the display outright. This is
        //    a size comparison, not a position one — it does not need align,
        //    so it runs whether or not align was set above — and it fires
        //    only when the text itself would not fit the panel at any
        //    position, not merely because of where this particular anchor
        //    happens to place it. The device clips at the display edge and
        //    still returns 200 OK, so a long CI message loses its tail — or,
        //    when centred, both its head and its tail.
        if let ElementKind::Text(text) = &element.kind {
            let estimated =
                (text.text.as_str().chars().count() as f32 * px_per_char(text.font)).round() as i16;

            if text.scroll_rate.is_none() && estimated > width {
                warnings.push(format!(
                    "element `{id}`'s text is about {estimated}px wide in font \
                     {:?}, which does not fit the {screen_name} display's {width}px; \
                     the device clips silently. Use --width and --scroll-rate to \
                     scroll it, or a smaller --font.",
                    text.font
                ));
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::bounds_warnings;
    use crate::device::{
        Align, AppName, DisplayElement, DisplayElements, Font, Screen, TextElement,
    };

    /// A short, deliberately narrow message so width never triggers a warning
    /// on its own — these cases are about position, not overflow.
    fn at(x: i16, y: i16, screen: Screen, align: Align) -> DisplayElements {
        text_at("hi", Font::Tiny, x, y, screen, align)
    }

    fn text_at(
        message: &str,
        font: Font,
        x: i16,
        y: i16,
        screen: Screen,
        align: Align,
    ) -> DisplayElements {
        let text = TextElement::new(message, font).unwrap();
        let element = DisplayElement::builder("message")
            .unwrap()
            .at(x, y)
            .screen(screen)
            .align(align)
            .text(text);
        DisplayElements::new(AppName::new("busy").unwrap())
            .unwrap()
            .element(element)
    }

    #[test]
    fn the_centred_default_position_is_quiet() {
        // busy text "hi" with no flags: the shipping default must not warn.
        assert!(bounds_warnings(&at(36, 8, Screen::Front, Align::Center)).is_empty());
        assert!(bounds_warnings(&at(80, 40, Screen::Back, Align::Center)).is_empty());
    }

    #[test]
    fn an_anchor_past_the_display_warns() {
        let warnings = bounds_warnings(&at(72, 0, Screen::Front, Align::TopLeft));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("message"),
            "should name the element id"
        );
        assert!(warnings[0].contains("72x16"), "should state the bounds");
    }

    #[test]
    fn a_negative_anchor_warns() {
        assert_eq!(
            bounds_warnings(&at(-1, 0, Screen::Front, Align::TopLeft)).len(),
            1
        );
        assert_eq!(
            bounds_warnings(&at(0, -1, Screen::Front, Align::TopLeft)).len(),
            1
        );
    }

    #[test]
    fn the_back_display_is_larger() {
        assert!(bounds_warnings(&at(100, 40, Screen::Back, Align::TopLeft)).is_empty());
        assert_eq!(
            bounds_warnings(&at(100, 40, Screen::Front, Align::TopLeft)).len(),
            1
        );
        assert_eq!(
            bounds_warnings(&at(160, 0, Screen::Back, Align::TopLeft)).len(),
            1
        );
    }

    // The cases the anchor-point check alone is blind to. Measured on real
    // hardware: at (0,0) these render a completely blank screen while the
    // device still returns 200 OK.
    #[test]
    fn a_right_anchor_at_x_zero_warns_even_though_the_point_is_in_bounds() {
        for align in [Align::TopRight, Align::MidRight, Align::BottomRight] {
            let warnings = bounds_warnings(&at(0, 8, Screen::Front, align));
            assert_eq!(
                warnings.len(),
                1,
                "{align:?} at x=0 should warn exactly once, got {warnings:?}"
            );
            assert!(
                warnings[0].contains("off the left"),
                "{align:?} at x=0 should warn, got {warnings:?}"
            );
        }
    }

    #[test]
    fn a_bottom_anchor_at_y_zero_warns_even_though_the_point_is_in_bounds() {
        for align in [Align::BottomLeft, Align::BottomMid, Align::BottomRight] {
            let warnings = bounds_warnings(&at(36, 0, Screen::Front, align));
            assert_eq!(
                warnings.len(),
                1,
                "{align:?} at y=0 should warn exactly once, got {warnings:?}"
            );
            assert!(
                warnings[0].contains("off the top"),
                "{align:?} at y=0 should warn, got {warnings:?}"
            );
        }
    }

    #[test]
    fn an_invisible_element_gets_one_warning_not_a_contradictory_second_one() {
        // Short text ("hi") at x=0 with `top_right`: the anchor-direction
        // check already says this renders nothing. It must not also claim,
        // falsely, that the text doesn't fit -- a few px easily fits in 72px.
        // Regression case for the bug where the width check's own span math
        // (not the text's actual width) triggered a second, false warning.
        let warnings = bounds_warnings(&at(0, 8, Screen::Front, Align::TopRight));
        assert_eq!(warnings.len(), 1, "got {warnings:?}");
        assert!(warnings[0].contains("off the left"));
    }

    #[test]
    fn a_left_anchor_at_x_zero_is_quiet() {
        // top_left at (0,0) is the one that renders fine — it must not warn.
        assert!(bounds_warnings(&at(0, 0, Screen::Front, Align::TopLeft)).is_empty());
    }

    #[test]
    fn text_too_wide_for_the_display_warns() {
        // 23 chars in `large` inks past the right edge; measured on hardware,
        // the device clips silently and returns 200.
        let warnings = bounds_warnings(&text_at(
            "Deployment completed OK",
            Font::Large,
            36,
            8,
            Screen::Front,
            Align::Center,
        ));
        assert_eq!(warnings.len(), 1, "got {warnings:?}");
        assert!(warnings[0].contains("clips silently"), "got {warnings:?}");
        assert!(
            warnings[0].contains("--scroll-rate"),
            "should point at the fix, got {warnings:?}"
        );
    }

    #[test]
    fn missing_align_does_not_suppress_the_width_warning() {
        // No `.align(..)` call at all: the align-direction check (case 2) has
        // nothing to check, but the width check (case 3) is independent of
        // align and must still run -- a payload built from a template file
        // that omits align should still get an overflow warning.
        let text = TextElement::new("Deployment completed OK", Font::Large).unwrap();
        let element = DisplayElement::builder("message")
            .unwrap()
            .at(36, 8)
            .screen(Screen::Front)
            .text(text);
        let payload = DisplayElements::new(AppName::new("busy").unwrap())
            .unwrap()
            .element(element);
        let warnings = bounds_warnings(&payload);
        assert_eq!(warnings.len(), 1, "got {warnings:?}");
        assert!(warnings[0].contains("clips silently"), "got {warnings:?}");
    }

    #[test]
    fn text_that_fits_is_quiet() {
        // Measured: "Hello, World!" in `large` inks 70px, inside the 72px front.
        assert!(
            bounds_warnings(&text_at(
                "Hello, World!",
                Font::Large,
                36,
                8,
                Screen::Front,
                Align::Center
            ))
            .is_empty()
        );
    }

    #[test]
    fn a_scrolling_element_is_never_warned_about_for_width() {
        let text = TextElement::new("Deployment completed OK", Font::Large)
            .unwrap()
            .scroll_rate(600);
        let element = DisplayElement::builder("message")
            .unwrap()
            .at(36, 8)
            .screen(Screen::Front)
            .align(Align::Center)
            .text(text);
        let payload = DisplayElements::new(AppName::new("busy").unwrap())
            .unwrap()
            .element(element);
        assert!(
            bounds_warnings(&payload).is_empty(),
            "scrolling is the intended fix for overflow, not a defect"
        );
    }
}
