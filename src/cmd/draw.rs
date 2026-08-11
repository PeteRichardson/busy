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

/// Which command is resolving a name — `run` shares `resolve`/`run` with
/// `Command::Draw` entirely (see `run`'s doc comment below), but the two
/// commands are not interchangeable from the user's point of view: `run` has
/// no `--file`, and a bare `busy template run` with no name is not the same
/// mistake as a bare `busy draw`. Threaded through only to phrase the "no
/// name" error against the command the user actually typed, rather than
/// always naming `draw` (and its `--file`, which `run` does not have) even
/// when `run` is what ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    Draw,
    TemplateRun,
}

impl Invocation {
    fn no_name_error(self) -> CliError {
        match self {
            Invocation::Draw => {
                CliError::usage("`busy draw` needs a name or --file; see `busy draw --help`")
            }
            Invocation::TemplateRun => CliError::usage(
                "`busy template run` needs a template name; see `busy template list`",
            ),
        }
    }
}

/// Resolve a name to a source.
///
/// 1. `shared/…` is the spec's reserved namespace for device built-ins.
/// 2. a local template directory under `root`.
/// 3. anything else is an asset in this application's directory. A message
///    here cannot be meant for an image, so it is the typo guard: report the
///    near-match instead of sending a doomed asset draw.
pub fn resolve(args: &DrawArgs, root: &Path, invocation: Invocation) -> Result<Resolved, CliError> {
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| invocation.no_name_error())?;

    let forced_stock = matches!(args.as_kind, Some(AsArg::Stock));
    let forced_image = matches!(args.as_kind, Some(AsArg::Image));
    let forced_template = matches!(args.as_kind, Some(AsArg::Template));

    // Rule 1: `shared/…` (or `--as stock`) dominates rule 2, so it is decided
    // first — but not returned yet, so the message guard below runs for it
    // too instead of only for the asset fallback.
    let is_stock = forced_stock || (args.as_kind.is_none() && name.starts_with("shared/"));
    let is_template = !is_stock
        && (forced_template || (!forced_image && root.join(name).join("template.toml").is_file()));

    if is_template {
        let template = Template::load(root, name)?;
        return Ok(Resolved::Template(Box::new(template)));
    }

    // Neither rule 1 (stock) nor rule 3 (asset) — the two non-template
    // resolutions — takes a message: an image has nothing to substitute it
    // into. Checked once, here, before either branch returns, rather than
    // only ahead of the asset fallback: a `shared/…` stock draw used to skip
    // this guard entirely and silently drop a second positional.
    if args.common.message.is_some() {
        let candidates = template::discover::list(root);
        let hint = match template::discover::suggest(name, &candidates) {
            Some(near) if near != name => format!(" Did you mean `{near}`?"),
            _ => String::new(),
        };
        return Err(CliError::usage(format!(
            "`{name}` resolved to an image, and images take no message.{hint}"
        )));
    }

    if is_stock {
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

/// Resolve `args` to a payload, apply the flags well-defined for its kind,
/// and either print it (`--dry-run`) or send it.
///
/// Shared by `Command::Draw` and `TemplateCmd::Run` — `run` is `draw` with
/// the name always read as a template (`args.as_kind` forced to
/// `AsArg::Template` by the caller) — so the two never diverge and a config
/// warning is never printed twice. `invocation` only affects error phrasing
/// (see `Invocation`'s doc comment); it changes nothing about resolution.
pub async fn run(
    args: &DrawArgs,
    settings: &Settings,
    file: &FileConfig,
    emitter: &Emitter,
    dry_run: bool,
    root: &Path,
    invocation: Invocation,
) -> Result<(), CliError> {
    let (payload, kind, template_context): (_, _, Option<(std::path::PathBuf, String)>) =
        match &args.file {
            Some(path) => (load_file(path)?, Kind::File, None),
            None => match resolve(args, root, invocation)? {
                Resolved::Template(template) => {
                    // `-` means stdin, exactly as it does for `busy text -`:
                    // command-surface spec §2.3 requires it here too, since
                    // `message` routinely arrives from `git log -1
                    // --format=%s`. `read_message` is a no-op for any other
                    // value, so this is safe to run unconditionally.
                    let message = args
                        .common
                        .message
                        .as_deref()
                        .map(crate::input::read_message)
                        .transpose()?;
                    let (vars, changed) =
                        template::bind_variables(message.as_deref(), &args.common.vars)?;
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
                    (
                        payload,
                        Kind::Template,
                        Some((template.dir.clone(), template.name.clone())),
                    )
                }
                resolved => {
                    let kind = if matches!(resolved, Resolved::Stock(_)) {
                        Kind::Stock
                    } else {
                        Kind::Image
                    };
                    (build_payload(args, settings, file, &resolved)?, kind, None)
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

    // Must run before the `--keep`-gated `clear()` below: a template with a
    // missing asset must not first wipe whatever is already on the panel and
    // only then refuse to draw.
    if let Some((dir, name)) = &template_context {
        crate::cmd::template::check_assets_present(&device, &payload, dir, name).await?;
    }

    if !args.common.delivery.keep {
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

    if let Some(percent) = args.common.opacity {
        let opacity = Opacity::new(percent)
            .map_err(|error| CliError::usage(format!("invalid --opacity: {error}")))?;
        element = element.opacity(opacity);
    }

    let screen = args
        .common
        .placement
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    let (default_x, default_y) = config::Defaults::position(screen);

    let id = args
        .common
        .delivery
        .id
        .clone()
        .unwrap_or_else(|| config::Defaults::IMAGE_ELEMENT_ID.to_owned());
    let id = crate::device::ElementId::new(id)
        .map_err(|error| CliError::usage(format!("invalid --id: {error}")))?;

    let mut builder = DisplayElement::builder(id)
        .map_err(|error| CliError::usage(error.to_string()))?
        .at(
            args.common.placement.x.unwrap_or(default_x),
            args.common.placement.y.unwrap_or(default_y),
        )
        .screen(screen)
        .align(config::resolve_align(args.common.placement.align, file)?);

    if let Some(seconds) = args.common.delivery.timeout {
        builder = builder.timeout_secs(seconds);
    }

    let priority_value = match &args.common.delivery.priority {
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

    if let Some(input) = &args.common.delivery.led {
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
