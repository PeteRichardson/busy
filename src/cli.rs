//! Command-line surface.
//!
//! Every option is `Option<T>` and no option carries a clap `default_value`.
//! Defaults live in `config::Defaults` so that "unset" stays distinguishable
//! from "explicitly set to the default", which the template layer needs.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "busy", version, about = "Draw on a BUSY Bar")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Draw a line of text
    Text(Box<TextArgs>),
    /// Draw an uploaded asset, a device built-in, or a raw payload
    Draw(Box<DrawArgs>),
    /// Manage this application's uploaded assets
    #[command(subcommand)]
    Asset(AssetCmd),
    /// Manage and run templates
    #[command(subcommand)]
    Template(TemplateCmd),
    /// Remove everything this application has drawn
    Clear,
}

#[derive(Subcommand, Debug)]
pub enum TemplateCmd {
    /// Write the shipped example templates into the template directory
    Init(TemplateInitArgs),
    /// List installed templates
    List,
    /// Show a template's description, elements, and variables
    Show(TemplateShowArgs),
    /// Check templates without contacting the device
    Validate(TemplateValidateArgs),
    /// Render a template and draw it
    Run(Box<TemplateRunArgs>),
}

#[derive(Args, Debug, Clone)]
pub struct TemplateInitArgs {
    /// Overwrite an example that already exists
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TemplateShowArgs {
    /// Template name
    pub name: String,
}

#[derive(Args, Debug, Clone)]
pub struct TemplateValidateArgs {
    /// Template name; every installed template when omitted
    pub name: Option<String>,
}

/// Fields shared by `busy draw` and `busy template run` — everything except
/// `name`.
///
/// Split out so the two commands cannot drift: `DrawArgs` is this plus its
/// own `name`, `--file`, and `--as`, and `TemplateRunArgs` is this plus its
/// own `name`. `run`'s name positional is always resolved as a template — no
/// `--file` (which would bypass templates entirely and draw a raw payload)
/// and no `--as` (which would parse and then be silently overridden) make
/// sense there, so neither is offered: there is exactly one definition of
/// the fields that ARE shared, rather than two structs whose fields must be
/// kept in step by hand.
///
/// `name` itself is declared separately on each of `DrawArgs` and
/// `TemplateRunArgs`, not here, because it is the one field that genuinely
/// means something different in each command: an asset name or a `shared/…`
/// built-in on `draw`, a template name and nothing else on `run`. Sharing it
/// anyway once meant `template run --help` advertised "Asset name, or a
/// `shared/…` device built-in" — help text that described `draw`'s behaviour
/// on a command that rejects a `shared/…` name as an unusable template name.
/// A duplicated field with genuinely divergent semantics is not the drift
/// risk this struct exists to prevent; a help string that contradicts the
/// command's own behaviour is worse.
#[derive(Args, Debug, Clone, Default)]
pub struct DrawCommon {
    /// Opacity, 0-100
    #[arg(short = 'o', long)]
    pub opacity: Option<u8>,

    /// Template variable, repeatable: --var key=value
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Optional message; binds to the `message` template variable. Use `-` to
    /// read it from stdin.
    pub message: Option<String>,

    #[command(flatten)]
    pub placement: PlacementArgs,

    #[command(flatten)]
    pub delivery: DeliveryArgs,
}

#[derive(Args, Debug, Clone, Default)]
pub struct DrawArgs {
    /// Asset name, or a `shared/…` device built-in
    pub name: Option<String>,

    #[command(flatten)]
    pub common: DrawCommon,

    /// Draw a raw DisplayElements payload from a file instead of a named thing
    #[arg(long, conflicts_with = "name")]
    pub file: Option<PathBuf>,

    /// Force how the name is interpreted, for pathological cases
    #[arg(long = "as", value_enum)]
    pub as_kind: Option<AsArg>,
}

/// `busy template run`'s arguments: its own `name`, plus `DrawCommon`, with
/// no `--file` and no `--as` — see `DrawCommon`'s doc comment.
#[derive(Args, Debug, Clone, Default)]
pub struct TemplateRunArgs {
    /// Template name
    pub name: Option<String>,

    #[command(flatten)]
    pub common: DrawCommon,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum AsArg {
    Image,
    Stock,
    Template,
}

#[derive(Subcommand, Debug)]
pub enum AssetCmd {
    /// Convert, fit, and upload a local image
    Upload(AssetUploadArgs),
    /// List this application's assets, read from the device
    List,
    /// Delete ALL of this application's assets
    Delete(AssetDeleteArgs),
}

#[derive(Args, Debug, Clone)]
pub struct AssetUploadArgs {
    /// Local image file. PNG, JPEG, or GIF; always stored as PNG.
    pub path: PathBuf,

    /// Panel to fit the image for. This is the *fit target*, not where the
    /// image is drawn — repeat `--screen` on `busy draw` to render it there.
    #[arg(short, long, value_enum)]
    pub screen: Option<ScreenArg>,
}

#[derive(Args, Debug, Clone)]
pub struct AssetDeleteArgs {
    /// Skip the confirmation prompt
    #[arg(short, long)]
    pub yes: bool,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Global")]
pub struct GlobalArgs {
    /// Device base URL
    #[arg(long, global = true, env = "BUSY_ADDR")]
    pub addr: Option<String>,

    /// API path prefix: `device` for a bar (/api), `cloud` for BUSY Cloud (/busybar)
    #[arg(long, global = true, value_enum)]
    pub api_prefix: Option<PrefixArg>,

    /// Access key. Prefer BUSY_TOKEN or the config file to keep it out of `ps`.
    #[arg(long, global = true, env = "BUSY_TOKEN", hide_env_values = true)]
    pub token: Option<String>,

    /// Application name that owns the drawn elements
    #[arg(long, global = true, env = "BUSY_APP")]
    pub app: Option<String>,

    /// HTTP request timeout in milliseconds (not how long the element stays up)
    #[arg(long, global = true)]
    pub http_timeout: Option<u64>,

    /// Directory holding template directories (default ~/.config/busy/templates)
    #[arg(long, global = true)]
    pub template_dir: Option<PathBuf>,

    /// Emit machine-readable JSON
    #[arg(short, long, global = true)]
    pub json: bool,

    /// Print the payload that would be sent, and send nothing
    #[arg(short = 'n', long, global = true)]
    pub dry_run: bool,

    /// Suppress warnings
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct TextArgs {
    /// The message. Use `-` to read it from stdin.
    pub message: String,

    #[command(flatten)]
    pub style: StyleArgs,

    #[command(flatten)]
    pub placement: PlacementArgs,

    #[command(flatten)]
    pub scroll: ScrollArgs,

    #[command(flatten)]
    pub delivery: DeliveryArgs,

    // `--until` only makes sense once a payload's lifetime is being parsed
    // (`text` owns that), so it lives directly on `TextArgs` rather than in
    // `DeliveryArgs`: `draw` flattens `DeliveryArgs` too, and must not
    // advertise a flag it does not accept (see issue #12). It is pinned to
    // the same "Delivery" help section as the rest of `DeliveryArgs` so the
    // split is invisible to `text --help`.
    /// Hide the element at this time: RFC 3339, or Unix seconds
    #[arg(short, long, conflicts_with = "timeout", help_heading = "Delivery")]
    pub until: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Style")]
pub struct StyleArgs {
    #[arg(short, long, value_enum)]
    pub font: Option<FontArg>,

    /// Colour: #RRGGBBAA, #RRGGBB, #RGB, 0x-prefixed, bare hex, or a name
    #[arg(short, long)]
    pub color: Option<String>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Placement")]
pub struct PlacementArgs {
    #[arg(short = 'x', long, allow_negative_numbers = true)]
    pub x: Option<i16>,

    #[arg(short = 'y', long, allow_negative_numbers = true)]
    pub y: Option<i16>,

    /// Anchor point, used together with -x/-y rather than instead of them
    #[arg(short, long, value_enum)]
    pub align: Option<AlignArg>,

    #[arg(short, long, value_enum)]
    pub screen: Option<ScreenArg>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Scrolling")]
pub struct ScrollArgs {
    /// Width of the label in pixels
    #[arg(short, long)]
    pub width: Option<u16>,

    /// Scroll rate in PIXELS PER MINUTE
    #[arg(short = 'r', long)]
    pub scroll_rate: Option<u32>,

    /// Milliseconds before scrolling starts
    #[arg(long)]
    pub scroll_start_delay: Option<u32>,

    /// Milliseconds between scroll cycles
    #[arg(long)]
    pub scroll_repeat_delay: Option<u32>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Delivery")]
pub struct DeliveryArgs {
    /// 1-100, or low|normal|high|urgent (10|50|95|100). A draw is accepted only
    /// when its priority is >= the running app's: built-ins are 10, an active
    /// BUSY work session is 90.
    #[arg(short, long)]
    pub priority: Option<String>,

    // The `--timeout`/`--until` conflict is declared only on `until` (in
    // `TextArgs`), not here: `DeliveryArgs` is also flattened into `draw`,
    // and clap's derive debug-asserts that every id named in
    // `conflicts_with` exists in that same subcommand — `draw` has no
    // `until` at all. clap's conflict checking is symmetric from a single
    // declared side, so declaring it once on `until` still makes `text
    // --timeout … --until …` an error either way round.
    /// Seconds the element stays on screen (0 = forever)
    #[arg(short, long)]
    pub timeout: Option<u32>,

    /// Blink the status LED this colour
    #[arg(short, long)]
    pub led: Option<String>,

    /// Element id, so repeat invocations update in place instead of accumulating
    #[arg(short, long)]
    pub id: Option<String>,

    /// Compose onto what is already on screen instead of replacing it
    #[arg(short, long)]
    pub keep: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum FontArg {
    Tiny,
    Small,
    Normal,
    Condensed,
    Bold,
    Large,
    ExtraLarge,
    Global,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum AlignArg {
    TopLeft,
    TopMid,
    TopRight,
    MidLeft,
    Center,
    MidRight,
    BottomLeft,
    BottomMid,
    BottomRight,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum ScreenArg {
    Front,
    Back,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum PrefixArg {
    Device,
    Cloud,
}
