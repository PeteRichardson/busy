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
    /// Remove everything this application has drawn
    Clear,
}

/// Filled in by Task 6, which wires `busy draw` up to assets and stock paths.
#[derive(Args, Debug, Clone, Default)]
pub struct DrawArgs {
    /// Asset name, or a `shared/…` device built-in
    pub name: Option<String>,
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

    /// Seconds the element stays on screen (0 = forever)
    #[arg(short, long, conflicts_with = "until")]
    pub timeout: Option<u32>,

    /// Hide the element at this time: RFC 3339, or Unix seconds
    #[arg(short, long)]
    pub until: Option<String>,

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
