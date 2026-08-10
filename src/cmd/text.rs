//! `busy text` — draw a single line of text.

use crate::cli::TextArgs;
use crate::config::{self, FileConfig, Settings};
use crate::device::{
    AppName, DisplayElement, DisplayElements, ElementId, Lifetime, Priority, Text, TextElement,
};
use crate::error::CliError;
use crate::{color, sanitize};

/// Build the wire payload. Pure: no I/O, no network, so `--dry-run` and the
/// real send are guaranteed to produce identical bytes.
pub fn build_payload(
    args: &TextArgs,
    settings: &Settings,
    file: &FileConfig,
    message: &str,
) -> Result<DisplayElements, CliError> {
    let sanitized = sanitize::to_ascii(message);
    if sanitized.text.is_empty() {
        return Err(CliError::usage(format!(
            "the message contains nothing printable on this device: {message:?}\n\
             The bar's fonts are bitmap ASCII, so emoji and other non-ASCII characters \
             are dropped. Supply at least one printable ASCII character."
        )));
    }

    let text = Text::new(sanitized.text)
        .map_err(|error| CliError::usage(format!("invalid message: {error}")))?;

    let mut element = TextElement::new(text, settings.font)
        .map_err(|error| CliError::usage(format!("invalid message: {error}")))?
        .color(settings.color);

    if let Some(width) = args.scroll.width {
        element = element.width(width);
    }
    if let Some(rate) = args.scroll.scroll_rate {
        element = element.scroll_rate(rate);
    }
    if let Some(delay) = args.scroll.scroll_start_delay {
        element = element.scroll_start_delay_ms(delay);
    }
    if let Some(delay) = args.scroll.scroll_repeat_delay {
        element = element.scroll_repeat_delay_ms(delay);
    }

    let id = args
        .delivery
        .id
        .clone()
        .unwrap_or_else(|| config::Defaults::ELEMENT_ID.to_owned());
    let id =
        ElementId::new(id).map_err(|error| CliError::usage(format!("invalid --id: {error}")))?;

    // Resolve the screen first: the default anchor position is the centre of
    // whichever display we are drawing to, and the two panels differ in size.
    let screen = args
        .placement
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    let (default_x, default_y) = config::Defaults::position(screen);

    let mut builder = DisplayElement::builder(id)
        .map_err(|error| CliError::usage(error.to_string()))?
        .at(
            args.placement.x.unwrap_or(default_x),
            args.placement.y.unwrap_or(default_y),
        )
        .screen(screen)
        .align(config::resolve_align(args.placement.align, file));

    match lifetime(args)? {
        Some(Lifetime::Timeout { timeout }) => builder = builder.timeout_secs(timeout),
        Some(Lifetime::DisplayUntil { display_until }) => {
            builder = builder.display_until(display_until)
        }
        None => {}
    }

    let priority_value = match &args.delivery.priority {
        Some(input) => config::parse_priority(input)?,
        None => settings.priority,
    };
    let priority = Priority::new(priority_value)
        .map_err(|error| CliError::usage(format!("invalid --priority: {error}")))?;

    let app = AppName::new(settings.app.clone())
        .map_err(|error| CliError::usage(format!("invalid --app: {error}")))?;

    let mut payload = DisplayElements::new(app)
        .map_err(|error| CliError::usage(error.to_string()))?
        .priority(priority)
        .element(builder.text(element));

    if let Some(input) = &args.delivery.led {
        payload = payload.led_notification_color(color::parse(input)?);
    }

    Ok(payload)
}

/// `timeout` and `display_until` are mutually exclusive; clap enforces that at
/// parse time, so at most one arm can fire here.
fn lifetime(args: &TextArgs) -> Result<Option<Lifetime>, CliError> {
    if let Some(seconds) = args.delivery.timeout {
        return Ok(Some(Lifetime::timeout_secs(seconds)));
    }
    match &args.delivery.until {
        Some(input) => Ok(Some(Lifetime::display_until(parse_until(input)?))),
        None => Ok(None),
    }
}

/// Accept Unix seconds or RFC 3339.
fn parse_until(input: &str) -> Result<u64, CliError> {
    if let Ok(seconds) = input.parse::<u64>() {
        return Ok(seconds);
    }
    let timestamp: jiff::Timestamp = input.parse().map_err(|error| {
        CliError::usage(format!(
            "invalid --until `{input}`: expected Unix seconds or an RFC 3339 timestamp \
             such as 2026-08-09T17:30:00Z ({error})"
        ))
    })?;
    let seconds = timestamp.as_second();
    u64::try_from(seconds)
        .map_err(|_| CliError::usage(format!("invalid --until `{input}`: predates 1970")))
}
