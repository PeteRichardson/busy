//! Layered configuration.
//!
//! Precedence, highest first: CLI flags, environment, config file, built-in
//! `Defaults`. `resolve` takes every layer as a value rather than reading the
//! process environment, which keeps the tests deterministic under parallelism
//! and leaves one obvious seam for the template layer to slot into later.
//!
//! `resolve` and `load_file` are wired into `main` as of Task 5.

use std::path::PathBuf;

use etcetera::BaseStrategy as _;
use serde::Deserialize;

use crate::cli::{AlignArg, FontArg, GlobalArgs, PrefixArg, ScreenArg, StyleArgs};
use crate::color;
use crate::device::{Align, Color, Font, Screen};

/// Built-in fallbacks. The single place a default value may be written.
pub struct Defaults;

impl Defaults {
    pub const ADDR: &'static str = "http://10.0.4.20";
    pub const APP: &'static str = "busy";
    pub const API_PREFIX: PrefixArg = PrefixArg::Device;
    pub const HTTP_TIMEOUT_MS: u64 = 5000;
    pub const FONT: Font = Font::Large;
    pub const COLOR: Color = Color::rgba(0xff, 0xff, 0xff, 0xff);
    pub const SCREEN: Screen = Screen::Front;
    /// 95 beats an active BUSY work session at 90. The device's own default is
    /// 50, which loses exactly when the user is at their desk.
    pub const PRIORITY: u8 = 95;
    pub const ELEMENT_ID: &'static str = "message";

    /// The device's own implicit anchor is `top_left` — measured, not
    /// documented upstream. We override it: centring the anchor on the middle
    /// of the display makes the zero-argument case (`busy text "hi"`) look
    /// deliberate rather than merely correct.
    pub const ALIGN: Align = Align::Center;

    /// Centre of a display, used as the default anchor position.
    ///
    /// Screen-dependent because the two panels differ: the front is 72x16 and
    /// the back is 160x80. A single constant pair would centre one and look
    /// accidental on the other.
    pub fn position(screen: Screen) -> (i16, i16) {
        match screen {
            Screen::Front => (36, 8),
            Screen::Back => (80, 40),
        }
    }
}

/// Values read from the environment. Constructed literally in tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub addr: Option<String>,
    pub token: Option<String>,
    pub app: Option<String>,
}

impl Env {
    /// Reads the real process environment, so no test may call this: the
    /// suite runs in parallel and every other test builds `Env` literally.
    pub fn from_process() -> Self {
        Self {
            addr: std::env::var("BUSY_ADDR").ok(),
            token: std::env::var("BUSY_TOKEN").ok(),
            app: std::env::var("BUSY_APP").ok(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub addr: Option<String>,
    pub app: Option<String>,
    pub token: Option<String>,
    pub http_timeout: Option<u64>,
    #[serde(default)]
    pub defaults: FileDefaults,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileDefaults {
    pub font: Option<String>,
    pub align: Option<String>,
    pub color: Option<String>,
    pub screen: Option<String>,
    pub priority: Option<u8>,
}

/// The file-and-default layer for every setting a command needs, computed
/// from `GlobalArgs`, `StyleArgs`, `Env`, and `FileConfig` alone. `resolve`
/// never sees `PlacementArgs` or `DeliveryArgs`, so `screen` and `priority`
/// here reflect only the config file and built-in defaults — callers must
/// still layer the `--screen`/`--priority` flags on top themselves, exactly
/// as they must for `align` via `resolve_align`.
#[derive(Debug, Clone)]
pub struct Settings {
    // These four fields feed the HTTP client that Task 7 introduced.
    pub addr: String,
    pub app: String,
    pub token: Option<String>,
    pub api_prefix: PrefixArg,
    pub http_timeout_ms: u64,
    pub font: Font,
    pub color: Color,
    // Stays dead through Task 7, which is what actually selects a screen on
    // the device; no test asserts on it either.
    #[cfg_attr(test, expect(dead_code, reason = "stays dead until Task 7 reads it"))]
    pub screen: Screen,
    pub priority: u8,
}

pub fn config_path() -> Option<PathBuf> {
    let strategy = etcetera::choose_base_strategy().ok()?;
    Some(strategy.config_dir().join("busy").join("config.toml"))
}

/// Load the config file, returning it alongside any warnings to print.
///
/// A missing file is not an error. An unreadable or malformed one is reported
/// as a warning and treated as absent, so a typo in the config never stops a
/// notification from reaching the bar.
pub fn load_file() -> (FileConfig, Vec<String>) {
    let mut warnings = Vec::new();

    let Some(path) = config_path() else {
        return (FileConfig::default(), warnings);
    };

    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (FileConfig::default(), warnings);
        }
        Err(error) => {
            warnings.push(format!("could not read {}: {error}", path.display()));
            return (FileConfig::default(), warnings);
        }
    };

    if let Some(mode) = world_readable_mode(&path) {
        warnings.push(format!(
            "{} is world-readable (mode {mode:o}); it may hold an access key",
            path.display()
        ));
    }

    match toml::from_str::<FileConfig>(&contents) {
        Ok(config) => (config, warnings),
        Err(error) => {
            warnings.push(format!("ignoring {}: {error}", path.display()));
            (FileConfig::default(), warnings)
        }
    }
}

#[cfg(unix)]
fn world_readable_mode(path: &std::path::Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    (mode & 0o004 != 0).then_some(mode & 0o777)
}

#[cfg(not(unix))]
fn world_readable_mode(_path: &std::path::Path) -> Option<u32> {
    None
}

pub fn resolve(
    global: &GlobalArgs,
    style: &StyleArgs,
    env: &Env,
    file: &FileConfig,
) -> Result<Settings, String> {
    let addr = global
        .addr
        .clone()
        .or_else(|| env.addr.clone())
        .or_else(|| file.addr.clone())
        .unwrap_or_else(|| Defaults::ADDR.to_owned());

    let app = global
        .app
        .clone()
        .or_else(|| env.app.clone())
        .or_else(|| file.app.clone())
        .unwrap_or_else(|| Defaults::APP.to_owned());

    let token = global
        .token
        .clone()
        .or_else(|| env.token.clone())
        .or_else(|| file.token.clone());

    let http_timeout_ms = global
        .http_timeout
        .or(file.http_timeout)
        .unwrap_or(Defaults::HTTP_TIMEOUT_MS);

    let font = match style.font {
        Some(font) => font_from_arg(font),
        None => match &file.defaults.font {
            Some(name) => parse_enum(name, "font")?,
            None => Defaults::FONT,
        },
    };

    let color = match &style.color {
        Some(input) => color::parse(input)?,
        None => match &file.defaults.color {
            Some(input) => color::parse(input)?,
            None => Defaults::COLOR,
        },
    };

    let screen = match &file.defaults.screen {
        Some(name) => parse_enum(name, "screen")?,
        None => Defaults::SCREEN,
    };

    let priority = match &file.defaults.priority {
        Some(value) => {
            if !(1..=100).contains(value) {
                return Err(format!(
                    "invalid priority `{value}` in the config file: expected 1-100"
                ));
            }
            *value
        }
        None => Defaults::PRIORITY,
    };

    Ok(Settings {
        addr,
        app,
        token,
        api_prefix: global.api_prefix.unwrap_or(Defaults::API_PREFIX),
        http_timeout_ms,
        font,
        color,
        screen,
        priority,
    })
}

/// Align is resolved separately because the flag lives on `PlacementArgs`
/// rather than `StyleArgs`. The API itself defines no default for it — the
/// device's implicit anchor is `top_left` — but we deliberately override
/// that with `Defaults::ALIGN` when neither the flag nor the config file
/// supplies one.
pub fn resolve_align(flag: Option<AlignArg>, file: &FileConfig) -> Align {
    if let Some(align) = flag {
        return align_from_arg(align);
    }
    file.defaults
        .align
        .as_deref()
        .and_then(|name| parse_enum::<Align>(name, "align").ok())
        .unwrap_or(Defaults::ALIGN)
}

pub fn parse_priority(input: &str) -> Result<u8, String> {
    match input.to_ascii_lowercase().as_str() {
        "low" => return Ok(10),
        "normal" => return Ok(50),
        "high" => return Ok(95),
        "urgent" => return Ok(100),
        _ => {}
    }

    let value: u8 = input.parse().map_err(|_| invalid_priority(input))?;
    if (1..=100).contains(&value) {
        Ok(value)
    } else {
        Err(invalid_priority(input))
    }
}

fn invalid_priority(input: &str) -> String {
    format!(
        "invalid priority `{input}`: expected 1-100, or one of low, normal, high, urgent \
         (10, 50, 95, 100)"
    )
}

/// Parse a snake_case enum value from the config file, using the same spellings
/// clap accepts on the command line so the two can never drift apart.
///
/// This routes through the CLI's own `*Arg` enums because the `busylib` types
/// do not implement `clap::ValueEnum` and are not ours to change.
fn parse_enum<T: FromArgName>(value: &str, label: &str) -> Result<T, String> {
    T::from_arg_name(value).ok_or_else(|| {
        format!(
            "invalid {label} `{value}` in the config file: expected one of {}",
            T::accepted().join(", ")
        )
    })
}

/// Implemented for `Font`, `Align`, and `Screen` below.
pub trait FromArgName: Sized {
    fn from_arg_name(value: &str) -> Option<Self>;
    fn accepted() -> Vec<&'static str>;
}

impl FromArgName for Font {
    fn from_arg_name(value: &str) -> Option<Self> {
        use clap::ValueEnum as _;
        FontArg::from_str(value, false).ok().map(font_from_arg)
    }
    fn accepted() -> Vec<&'static str> {
        vec![
            "tiny",
            "small",
            "normal",
            "condensed",
            "bold",
            "large",
            "extra_large",
            "global",
        ]
    }
}

impl FromArgName for Align {
    fn from_arg_name(value: &str) -> Option<Self> {
        use clap::ValueEnum as _;
        AlignArg::from_str(value, false).ok().map(align_from_arg)
    }
    fn accepted() -> Vec<&'static str> {
        vec![
            "top_left",
            "top_mid",
            "top_right",
            "mid_left",
            "center",
            "mid_right",
            "bottom_left",
            "bottom_mid",
            "bottom_right",
        ]
    }
}

impl FromArgName for Screen {
    fn from_arg_name(value: &str) -> Option<Self> {
        use clap::ValueEnum as _;
        ScreenArg::from_str(value, false).ok().map(screen_from_arg)
    }
    fn accepted() -> Vec<&'static str> {
        vec!["front", "back"]
    }
}

pub fn font_from_arg(arg: FontArg) -> Font {
    match arg {
        FontArg::Tiny => Font::Tiny,
        FontArg::Small => Font::Small,
        FontArg::Normal => Font::Normal,
        FontArg::Condensed => Font::Condensed,
        FontArg::Bold => Font::Bold,
        FontArg::Large => Font::Large,
        FontArg::ExtraLarge => Font::ExtraLarge,
        FontArg::Global => Font::Global,
    }
}

pub fn align_from_arg(arg: AlignArg) -> Align {
    match arg {
        AlignArg::TopLeft => Align::TopLeft,
        AlignArg::TopMid => Align::TopMid,
        AlignArg::TopRight => Align::TopRight,
        AlignArg::MidLeft => Align::MidLeft,
        AlignArg::Center => Align::Center,
        AlignArg::MidRight => Align::MidRight,
        AlignArg::BottomLeft => Align::BottomLeft,
        AlignArg::BottomMid => Align::BottomMid,
        AlignArg::BottomRight => Align::BottomRight,
    }
}

pub fn screen_from_arg(arg: ScreenArg) -> Screen {
    match arg {
        ScreenArg::Front => Screen::Front,
        ScreenArg::Back => Screen::Back,
    }
}

#[cfg(test)]
mod tests {
    use super::{Env, FileConfig, Settings, parse_priority, resolve};
    use crate::cli::{AlignArg, FontArg, GlobalArgs, PrefixArg, StyleArgs};
    use crate::device::{Align, Font};

    fn settings(global: GlobalArgs, style: StyleArgs, env: Env, file: FileConfig) -> Settings {
        resolve(&global, &style, &env, &file).expect("should resolve")
    }

    #[test]
    fn built_in_defaults_apply_when_nothing_is_set() {
        let resolved = settings(
            GlobalArgs::default(),
            StyleArgs::default(),
            Env::default(),
            FileConfig::default(),
        );
        assert_eq!(resolved.addr, "http://10.0.4.20");
        assert_eq!(resolved.app, "busy");
        assert_eq!(resolved.token, None);
        assert_eq!(resolved.api_prefix, PrefixArg::Device);
        assert_eq!(resolved.http_timeout_ms, 5000);
        assert_eq!(resolved.font, Font::Large);
        assert_eq!(resolved.color.to_string(), "#FFFFFFFF");
        assert_eq!(resolved.priority, 95, "must beat a work session at 90");
    }

    #[test]
    fn the_config_file_beats_the_built_in_defaults() {
        let file = toml::from_str::<FileConfig>(
            // A plain `r#"..."#` raw string would close early: the body
            // contains the literal text `"#00ff00ff"`, whose `"#` matches
            // the single-hash terminator. Use two hashes to avoid the clash.
            r##"
            addr = "http://192.168.1.9"
            app = "ci"
            [defaults]
            font = "small"
            align = "center"
            color = "#00ff00ff"
            priority = 50
            "##,
        )
        .expect("valid config");

        let resolved = settings(
            GlobalArgs::default(),
            StyleArgs::default(),
            Env::default(),
            file,
        );
        assert_eq!(resolved.addr, "http://192.168.1.9");
        assert_eq!(resolved.app, "ci");
        assert_eq!(resolved.font, Font::Small);
        assert_eq!(resolved.color.to_string(), "#00FF00FF");
        assert_eq!(resolved.priority, 50);
    }

    #[test]
    fn the_environment_beats_the_config_file() {
        let file = toml::from_str::<FileConfig>(r#"addr = "http://from-file""#).unwrap();
        let env = Env {
            addr: Some("http://from-env".into()),
            ..Env::default()
        };
        let resolved = settings(GlobalArgs::default(), StyleArgs::default(), env, file);
        assert_eq!(resolved.addr, "http://from-env");
    }

    #[test]
    fn flags_beat_everything() {
        let file = toml::from_str::<FileConfig>(
            r#"
            addr = "http://from-file"
            [defaults]
            font = "tiny"
            "#,
        )
        .unwrap();
        let env = Env {
            addr: Some("http://from-env".into()),
            ..Env::default()
        };
        let global = GlobalArgs {
            addr: Some("http://from-flag".into()),
            ..GlobalArgs::default()
        };
        let style = StyleArgs {
            font: Some(FontArg::Bold),
            ..StyleArgs::default()
        };

        let resolved = settings(global, style, env, file);
        assert_eq!(resolved.addr, "http://from-flag");
        assert_eq!(resolved.font, Font::Bold);
    }

    #[test]
    fn align_resolves_from_the_flag_then_the_file_then_nothing() {
        let empty = FileConfig::default();
        // Deliberately distinct from both `Defaults::ALIGN` (Center) and the
        // `top_left` used for the flag case below, so each assertion actually
        // pins which layer won rather than being trivially satisfied by the
        // default.
        let with_align = toml::from_str::<FileConfig>(
            r#"
            [defaults]
            align = "bottom_mid"
            "#,
        )
        .unwrap();

        // No default at the API level: the device's implicit anchor is
        // `top_left`, but we deliberately override it with `Defaults::ALIGN`
        // when nothing else sets it.
        assert_eq!(super::resolve_align(None, &empty), Align::Center);
        assert_eq!(
            super::resolve_align(None, &with_align),
            Align::BottomMid,
            "the config file must be consulted when no flag is given"
        );
        assert_eq!(
            super::resolve_align(Some(AlignArg::TopLeft), &with_align),
            Align::TopLeft,
            "the flag must win over the config file"
        );
    }

    #[test]
    fn priority_accepts_numbers_and_names() {
        assert_eq!(parse_priority("1").unwrap(), 1);
        assert_eq!(parse_priority("100").unwrap(), 100);
        assert_eq!(parse_priority("low").unwrap(), 10);
        assert_eq!(parse_priority("normal").unwrap(), 50);
        assert_eq!(parse_priority("high").unwrap(), 95);
        assert_eq!(parse_priority("urgent").unwrap(), 100);
        assert_eq!(parse_priority("HIGH").unwrap(), 95);
    }

    #[test]
    fn priority_rejects_out_of_range_and_nonsense() {
        for bad in ["0", "101", "-1", "", "medium"] {
            assert!(parse_priority(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn a_bad_colour_in_the_config_file_is_reported() {
        let file = toml::from_str::<FileConfig>(
            r#"
            [defaults]
            color = "chartreuse"
            "#,
        )
        .unwrap();
        let error = resolve(
            &GlobalArgs::default(),
            &StyleArgs::default(),
            &Env::default(),
            &file,
        )
        .expect_err("should reject");
        assert!(error.contains("chartreuse"), "got {error:?}");
    }
}
