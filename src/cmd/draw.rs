//! `busy draw` — put a named thing on the bar.
//!
//! The unifying idea is that `draw` takes a name which expands to
//! `DisplayElements`. A name expands to a single `ImageElement`, a device
//! stock path, or — resolution rule 2, below — a rendered template.

use std::path::Path;

use crate::cli::{AsArg, DrawArgs};
use crate::cmd::template::SANITIZED_VARIABLE_WARNING;
use crate::config::{self, FileConfig, Settings};
use crate::device::{
    AssetPath, Device, DisplayElement, DisplayElements, ImageElement, Opacity, StockPath,
};
use crate::error::CliError;
use crate::output::Emitter;
use crate::overrides::{self, Kind};
use crate::template::{self, Template};

/// What a `draw` name turned out to mean.
#[derive(Debug, Clone)]
pub enum Resolved {
    Asset(AssetPath),
    Stock(StockPath),
    Template(Box<Template>),
}

/// Resolve a name to a source.
///
/// 1. `shared/…` is the spec's reserved namespace for device built-ins.
/// 2. a local template directory under `root`.
/// 3. anything else is an asset in this application's directory. A message
///    here cannot be meant for an image, so it is the typo guard: report the
///    near-match instead of sending a doomed asset draw.
pub fn resolve(args: &DrawArgs, root: &Path) -> Result<Resolved, CliError> {
    let name = args.name.as_deref().ok_or_else(|| {
        CliError::usage("`busy draw` needs a name or --file; see `busy draw --help`")
    })?;

    let forced_stock = matches!(args.as_kind, Some(AsArg::Stock));
    let forced_image = matches!(args.as_kind, Some(AsArg::Image));
    let forced_template = matches!(args.as_kind, Some(AsArg::Template));

    if forced_stock || (args.as_kind.is_none() && name.starts_with("shared/")) {
        let stock = StockPath::new(name).map_err(|error| {
            CliError::usage(format!(
                "`{name}` is not a valid stock path: {error}. Device built-ins look like \
                 `shared/checkmark_front_8x8.image`."
            ))
        })?;
        return Ok(Resolved::Stock(stock));
    }

    if forced_template || (!forced_image && root.join(name).join("template.toml").is_file()) {
        let template = Template::load(root, name)?;
        return Ok(Resolved::Template(Box::new(template)));
    }

    if args.message.is_some() {
        let candidates = template::discover::list(root);
        let hint = match template::discover::suggest(name, &candidates) {
            Some(near) => format!(" Did you mean `{near}`?"),
            None => String::new(),
        };
        return Err(CliError::usage(format!(
            "`{name}` resolved to an image, and images take no message.{hint}"
        )));
    }

    let asset = AssetPath::new(name)
        .map_err(|error| CliError::usage(format!("`{name}` is not a valid asset name: {error}")))?;
    Ok(Resolved::Asset(asset))
}

/// Resolve `args` to a payload, apply the flags well-defined for its kind,
/// and either print it (`--dry-run`) or send it.
///
/// Shared by `Command::Draw` and `TemplateCmd::Run` — `run` is `draw` with
/// the name always read as a template (`args.as_kind` forced to
/// `AsArg::Template` by the caller) — so the two never diverge and a config
/// warning is never printed twice.
pub async fn run(
    args: &DrawArgs,
    settings: &Settings,
    file: &FileConfig,
    emitter: &Emitter,
    dry_run: bool,
    root: &Path,
) -> Result<(), CliError> {
    let (payload, kind) = match &args.file {
        Some(path) => (load_file(path)?, Kind::File),
        None => match resolve(args, root)? {
            Resolved::Template(template) => {
                let (vars, changed) =
                    template::bind_variables(args.message.as_deref(), &args.vars)?;
                if changed {
                    emitter.warn(SANITIZED_VARIABLE_WARNING);
                }

                // Catch a missing variable here, against the static
                // analysis, rather than letting `render` hit it mid-
                // substitution: minijinja's own "undefined value" names
                // neither the variable nor how to supply it, and forgetting
                // one (`message`, above all) is the most common mistake a
                // template user makes.
                template.check_required_variables(&vars)?;

                let rendered = template.render(&vars)?;
                let payload = rendered.into_payload(&settings.app)?;

                let report = template::validate::offline(&payload, &template.dir);
                if !report.is_ok() {
                    return Err(CliError::usage(report.errors.join("\n")));
                }
                for warning in &report.warnings {
                    emitter.warn(warning);
                }
                (payload, Kind::Template)
            }
            resolved => {
                let kind = if matches!(resolved, Resolved::Stock(_)) {
                    Kind::Stock
                } else {
                    Kind::Image
                };
                (build_payload(args, settings, file, &resolved)?, kind)
            }
        },
    };

    overrides::reject_vars_unless_template(args, kind)?;
    let payload = overrides::apply(payload, args, kind)?;

    for warning in crate::validate::bounds_warnings(&payload) {
        emitter.warn(&warning);
    }

    if dry_run {
        return emitter.dry_run(&payload);
    }

    let device = Device::connect(settings)?;
    if !args.delivery.keep {
        device.clear().await?;
    }
    device.draw(&payload).await?;

    emitter.success("drawn", Some(&payload))
}

/// Build the wire payload for an asset or stock draw. Pure: no I/O, no
/// network, so `--dry-run` and the real send are guaranteed to produce
/// identical bytes.
///
/// Never called with `Resolved::Template`: `run` renders a template on its
/// own branch, well before this function, because a template arrives with
/// its elements already decided rather than built from these flags. Private
/// — `run`, in this module, is the only caller (this crate has no library
/// target; nothing outside `cmd::draw` can reach this function at all) — so
/// the `unreachable!()` below is exactly that: unreachable, not merely
/// undocumented API misuse from some other module.
fn build_payload(
    args: &DrawArgs,
    settings: &Settings,
    file: &FileConfig,
    resolved: &Resolved,
) -> Result<DisplayElements, CliError> {
    // `draw`'s delivery args have no `until` field at all (see `cli.rs`), so
    // there is nothing to reject here: clap's own "unexpected argument"
    // covers both the --file and the named-draw paths uniformly, before
    // either one reaches this function.
    let mut element = match resolved {
        Resolved::Asset(path) => ImageElement::asset(path.clone()),
        Resolved::Stock(path) => ImageElement::stock(path.clone()),
        Resolved::Template(_) => {
            unreachable!("run() renders a template on its own branch before calling build_payload")
        }
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
        .unwrap_or_else(|| config::Defaults::IMAGE_ELEMENT_ID.to_owned());
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
