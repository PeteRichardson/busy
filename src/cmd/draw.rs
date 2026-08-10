//! `busy draw` — put a named thing on the bar.
//!
//! The unifying idea is that `draw` takes a name which expands to
//! `DisplayElements`. In this phase a name expands to a single `ImageElement`;
//! Phase 4 inserts template lookup between the stock and asset rules, so keep
//! `resolve` shaped for that insertion rather than restructuring it later.

use crate::cli::{AsArg, DrawArgs};
use crate::config::{self, FileConfig, Settings};
use crate::device::{AssetPath, DisplayElement, DisplayElements, ImageElement, Opacity, StockPath};
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

    let asset = AssetPath::new(name)
        .map_err(|error| CliError::usage(format!("`{name}` is not a valid asset name: {error}")))?;
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
        let opacity = Opacity::new(percent)
            .map_err(|error| CliError::usage(format!("invalid --opacity: {error}")))?;
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
        .align(config::resolve_align(args.placement.align, file)?);

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
