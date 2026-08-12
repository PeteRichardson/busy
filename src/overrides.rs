//! Which flags apply to which kind of draw.
//!
//! One table, in one place. Before this existed, the same judgment lived in
//! four hand-written rejection chains across `main.rs` and `cmd/draw.rs`.
//!
//! The rule: a payload-level flag overrides, because a payload has exactly one
//! priority and one LED colour. A per-element flag is refused whenever the
//! payload may hold several elements, because there is no principled way to
//! pick which one it applies to — applying it to the first, or to all of them,
//! are both defensible and neither is obviously right.

use crate::cli::DrawArgs;
use crate::device::{DisplayElements, Priority};
use crate::error::CliError;

/// What a draw turned out to be drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Stock,
    /// A `.anim`, whether from this app's assets or from `shared/…`. Built
    /// from flags exactly as an image is, but it is the only kind that can
    /// honour `--loop` and `--section`.
    Animation,
    Template,
    File,
}

impl Kind {
    /// Whether this kind builds its element from CLI flags, or arrives with
    /// its elements already decided.
    fn is_prebuilt(self) -> bool {
        matches!(self, Kind::Template | Kind::File)
    }

    fn describe(self) -> &'static str {
        match self {
            Kind::Image | Kind::Stock => "an image draw",
            Kind::Animation => "an animation draw",
            Kind::Template => "a template",
            Kind::File => "a --file payload",
        }
    }
}

/// Apply the flags that are well-defined for `kind`, and refuse the rest.
pub fn apply(
    mut payload: DisplayElements,
    args: &DrawArgs,
    kind: Kind,
) -> Result<DisplayElements, CliError> {
    if !kind.is_prebuilt() {
        // An image draw builds its element from these, so there is nothing to
        // refuse and nothing to override here.
        return Ok(payload);
    }

    let per_element: [(&str, bool); 7] = [
        ("-x/--x", args.common.placement.x.is_some()),
        ("-y/--y", args.common.placement.y.is_some()),
        ("--align", args.common.placement.align.is_some()),
        ("--screen", args.common.placement.screen.is_some()),
        ("--timeout", args.common.delivery.timeout.is_some()),
        ("--opacity", args.common.opacity.is_some()),
        ("--id", args.common.delivery.id.is_some()),
    ];

    for (flag, given) in per_element {
        if given {
            return Err(rejected(flag, kind));
        }
    }

    if let Some(input) = &args.common.delivery.priority {
        let value = crate::config::parse_priority(input)?;
        let priority = Priority::new(value)
            .map_err(|error| CliError::usage(format!("invalid --priority: {error}")))?;
        payload.priority = Some(priority);
    }

    if let Some(input) = &args.common.delivery.led {
        payload.led_notification_color = Some(crate::color::parse(input)?);
    }

    Ok(payload)
}

/// `--loop` and `--section` only mean something for an animation.
///
/// A template or a payload file may well contain animation elements, but each
/// carries its own `loop` and `section`, and the flag cannot say which element
/// it meant — the same reason the per-element flags above are refused there.
pub fn reject_animation_flags_unless_animation(
    args: &DrawArgs,
    kind: Kind,
) -> Result<(), CliError> {
    if kind == Kind::Animation {
        return Ok(());
    }

    let flags = [
        ("--loop", args.common.repeat),
        ("--section", args.common.section.is_some()),
    ];

    for (flag, given) in flags {
        if !given {
            continue;
        }
        let advice = match kind {
            Kind::Template | Kind::File => {
                "an animation element inside it carries its own, and this cannot say \
                 which element it meant."
            }
            _ => "only a `.anim` animation has frames to play.",
        };
        return Err(CliError::usage(format!(
            "{flag} cannot be used with {}: {advice}",
            kind.describe()
        )));
    }

    Ok(())
}

/// `--var` only means something for a template.
pub fn reject_vars_unless_template(args: &DrawArgs, kind: Kind) -> Result<(), CliError> {
    if !args.common.vars.is_empty() && kind != Kind::Template {
        return Err(CliError::usage(format!(
            "--var cannot be used with {}: variables are substituted into a template, \
             and this is not one.",
            kind.describe()
        )));
    }
    Ok(())
}

fn rejected(flag: &str, kind: Kind) -> CliError {
    let advice = match kind {
        Kind::Template => {
            "it applies to a single element, but a template may hold several. Edit the \
             template, or expose the value as a `{{ variable }}`."
        }
        _ => {
            "it applies to a single element, but a payload file may hold several. Edit \
             the file's own fields instead."
        }
    };
    CliError::usage(format!(
        "{flag} cannot be used with {}: {advice}",
        kind.describe()
    ))
}
