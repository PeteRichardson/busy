# `busy` CLI Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a releasable `busy` binary that draws text on a BUSY Bar — `busy text "Hello, World!"` — with the full option surface, layered configuration, `--dry-run`/`--json`, and explicit handling of every way the device accepts a request and renders nothing.

**Architecture:** A single crate. `src/device.rs` is the only module that names `busylib`; it owns the client and re-exports the model and value types the rest of the CLI uses, so an upstream reshuffle is a one-file fix. Every CLI option is `Option<T>` and is resolved through one precedence chain (flags → env → config file → built-in `Defaults`), which is the only way a later template layer can supply values that flags still override. Commands build a `DisplayElements` payload, validate it locally, and either print it (`--dry-run`) or send it.

**Tech Stack:** Rust 2024, `busylib` 0.0.11 (HTTP client), `clap` 4 (derive), `tokio` (current-thread runtime), `serde`/`serde_json`/`toml`, `thiserror`, `etcetera` (XDG paths), `jiff` (RFC 3339). Tests: `insta` (golden payloads), `wiremock` (HTTP), `assert_cmd` + `predicates` (CLI surface).

**Scope:** Phases 1–2 of `docs/busy-cli-architecture.md`. Phase 3 (assets and `busy draw`), Phase 4 (templates), and Phase 5 (polish) get their own plans. Nothing here builds images, templates, or asset sync.

**Source documents:**
- `docs/specs/2026-08-09-busy-cli-ux-design.md` — the command surface (authoritative on what the user types)
- `docs/busy-cli-architecture.md` — device behaviour, failure modes, config precedence
- `docs/specs/openapi.yaml` — the vendored API document, v25.0.0

## Global Constraints

Every task's requirements implicitly include this section.

- **Package `busy-cli`, binary `busy`.** The crate names `busybar`/`busylib` and the binary name `busybar` are taken; `busy` is free.
- **`busylib = "=0.0.11"`, pinned exactly, with DEFAULT FEATURES.** Do **not** pass `default-features = false, features = ["reqwest"]` — see "Upstream defect" below. Upgrade deliberately, reading the changelog.
- **Only `src/device.rs` may write `use busylib::…`.** Every other module imports the model and value types from `crate::device`, which re-exports them.
- **Every CLI option is `Option<T>`. Never use clap's `default_value`.** Defaults live in exactly one place, `config::Defaults`. This is what makes "unset" distinguishable from "explicitly set to the default value", which the template layer in a later plan depends on.
- **A subcommand is always required.** `busy` alone prints help and exits 2. There is no implicit default command, no bare top-level positional, and no `-m`/`--message`.
- **Two distinct timeouts:** `--http-timeout <ms>` is the HTTP request timeout; `--timeout <secs>` is how long an element stays on screen. Never name them alike.
- **Text is printable ASCII only** — `^[\x20-\x7E]+$`, `minLength: 1`. Sanitize before sending; never surface a raw regex-mismatch error.
- **`application_name` and element `id`** match `^[a-zA-Z0-9._-]+$` — no spaces.
- **Priority is 1–100; the CLI default is 95**, which beats an active work session (90). The device's own default is 50, which loses.
- **Colour is `#RRGGBBAA` on the wire, uppercase** (`busylib`'s `Display` uses `{:02X}`). The CLI parses leniently and constructs `Color::rgba`.
- **Exit codes:** 0 success, 1 runtime failure, 2 usage error (clap's default).
- Rust edition 2024. `busylib` requires 1.86; the repo toolchain is 1.97.1.

### Upstream defect — read before Task 1

`docs/busy-cli-architecture.md` §1 instructs disabling the `ws` feature. **That configuration does not compile.** `busylib` 0.0.11 declares `mod streaming;` unconditionally in `src/api/mod.rs:15`, but `src/api/streaming.rs` uses `crate::proto`, `prost::Message`, and `Error::DecodeProto`, all of which exist only under the `ws` feature. Building with `--no-default-features --features reqwest` fails with three errors (`E0433`, `E0432`, `E0599`).

Use default features. The cost is that `tokio-tungstenite`, `prost`, and `futures-util` enter the dependency tree unused.

The upstream fix is two `#[cfg(feature = "ws")]` attributes in `src/api/mod.rs` — on `mod streaming;` (line 15) and on `pub use streaming::{StatusStream, Streaming};` (line 29). Per the architecture doc's dependency policy ("gaps found along the way are worth contributing upstream"), file this at `github.com/foresterre/busybar-rust`. Revisit the feature flags when a release carries the fix.

---

## File Structure

```
busy/
├── Cargo.toml
├── docs/                       # already present; not modified by this plan
├── scripts/probe-device.sh     # already present
└── src/
    ├── main.rs                 # entry point, runtime, top-level error reporting, exit codes
    ├── cli.rs                  # all clap derive definitions and arg groups
    ├── config.rs               # Defaults, FileConfig, Env, and the precedence resolver
    ├── color.rs                # lenient colour parsing -> device::Color
    ├── sanitize.rs             # Unicode -> printable ASCII, with a changed flag
    ├── validate.rs             # local bounds checks that warn before sending
    ├── error.rs                # CliError, including the priority-conflict guidance
    ├── output.rs               # human / --json / --dry-run emission
    ├── device.rs               # THE ONLY module importing busylib
    └── cmd/
        ├── mod.rs
        ├── text.rs             # busy text
        └── clear.rs            # busy clear
```

Responsibilities are deliberately narrow so each file stays reviewable in one sitting. `color.rs`, `sanitize.rs`, `validate.rs`, and `config.rs` are pure — no I/O, no async, no network — which is what makes the bulk of the test suite fast and deterministic.

**Note on the `device.rs` rule.** The architecture doc §4 says `device.rs` is "THE ONLY module that imports busylib", but §7 also has templates deserializing directly into `busylib::model::assets::DisplayElements`. Taken literally those conflict. The resolution used here: `device.rs` owns the entire *client* surface (`Client`, `ClientBuilder`, `ApiPrefix`, `ReqwestHttpTransport`, `Error`) **and** re-exports the model and value types via `pub use`. Other modules write `use crate::device::{DisplayElements, Font};`, never `use busylib::…`. This keeps the one-file-fix property that motivated the rule while remaining achievable.

---

## Task 1: Crate skeleton and command surface

Establishes the binary, the dependency set, and the clap structure. No device contact.

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/cli.rs`
- Create: `.gitignore`
- Test: `tests/cli_surface.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `cli::Cli` (clap `Parser`) with `Cli::global: GlobalArgs` and `Cli::command: Command`; the arg-group structs `GlobalArgs`, `StyleArgs`, `PlacementArgs`, `ScrollArgs`, `DeliveryArgs`, `TextArgs`; the enums `FontArg`, `AlignArg`, `ScreenArg`, `PrefixArg`. Every field is `Option<T>` except the boolean flags and `TextArgs::message`, which is a **required** `String`.

**Why `message` is required rather than `Option<String>`.** The Global Constraint that every option is `Option<T>` is about *options*, so that "unset" stays distinguishable from "set to the default". A positional has no default to be confused with. Making it required is also the only way `busy text` (no message) becomes a clap usage error exiting 2 — verified: an optional positional exits 0 and silently does nothing. `busy text -` still reads stdin, because `-` is a value like any other, and `busy text -- "-3 tests"` still passes a leading-dash message.

- [ ] **Step 1: Initialise the crate**

```bash
cd /Users/pete/projects/busy
cargo init --name busy-cli
```

Then replace the generated `Cargo.toml` with:

```toml
[package]
name = "busy-cli"
version = "0.1.0"
edition = "2024"
rust-version = "1.86"
description = "An ergonomic CLI for the BUSY Bar"
license = "MIT OR Apache-2.0"

[[bin]]
name = "busy"
path = "src/main.rs"

[dependencies]
# Pinned exactly: busylib reached 0.0.11 within two days of first publication and
# the module layout has already moved once. Default features are required — the
# reqwest-only combination does not compile in 0.0.11 (see Global Constraints).
busylib = "=0.0.11"
clap = { version = "4.6.6", features = ["derive", "env", "wrap_help"] }
etcetera = "0.11.0"
# `http` arrives transitively via busylib, but device.rs names StatusCode
# directly, so depend on it explicitly rather than relying on that.
http = "1.5.0"
jiff = "0.2.35"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.20"
# rt-multi-thread is for #[tokio::test] in the wiremock suites; the binary
# itself runs on the current-thread flavour.
tokio = { version = "1.53.1", features = ["rt", "rt-multi-thread", "macros", "io-std", "io-util"] }
toml = "1.1.4"

[dev-dependencies]
assert_cmd = "2.2.2"
insta = "1.48.0"
predicates = "3.1.4"
wiremock = "0.6.5"
```

And write `.gitignore`:

```
/target
```

- [ ] **Step 2: Write the shared test helpers**

Each file under `tests/` compiles as its own crate, so helpers can only be shared through a `common` module that each one declares. Every later task's tests use these; do not redefine them per file.

Create `tests/common/mod.rs`:

```rust
//! Helpers shared by the integration test crates.
//!
//! Each file in tests/ is a separate crate that compiles this whole module but
//! uses only part of it, so unused-item warnings here are expected rather than
//! a signal.
#![allow(dead_code)]

use assert_cmd::Command;
use wiremock::{MockServer, ResponseTemplate};

/// A `busy` invocation with a neutral environment, so a developer's own config
/// file and `BUSY_*` variables can never change what a test observes.
pub fn busy() -> Command {
    let mut command = Command::cargo_bin("busy").expect("binary `busy` should build");
    command
        .env_remove("BUSY_ADDR")
        .env_remove("BUSY_TOKEN")
        .env_remove("BUSY_APP")
        .env("XDG_CONFIG_HOME", "/nonexistent");
    command
}

/// `busy` pointed at a mock device.
pub fn busy_at(server: &MockServer) -> Command {
    let mut command = busy();
    command.args(["--addr", &server.uri()]);
    command
}

/// The body the device returns on success.
pub fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": "OK"}))
}
```

- [ ] **Step 3: Write the failing test**

Create `tests/cli_surface.rs`:

```rust
mod common;

use common::busy;
use predicates::str::contains;

#[test]
fn bare_invocation_prints_help_and_fails() {
    // Verified against clap 4.6: `arg_required_else_help` writes the help to
    // stderr and exits 2.
    busy()
        .assert()
        .failure()
        .code(2)
        .stderr(contains("Usage: busy"));
}

#[test]
fn text_without_a_message_is_a_usage_error() {
    busy().arg("text").assert().failure().code(2);
}

#[test]
fn text_with_a_message_parses() {
    // --dry-run is parsed but ignored until Task 5, and never contacts a device
    // in any task — which is what keeps this assertion stable as the tool grows.
    busy()
        .args(["--dry-run", "text", "Hello, World!"])
        .assert()
        .success();
}

#[test]
fn there_is_no_message_flag() {
    busy()
        .args(["text", "-m", "Hello"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn there_is_no_bare_top_level_positional() {
    busy().arg("Hello, World!").assert().failure().code(2);
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --test cli_surface`
Expected: FAIL — the binary does not yet accept a `text` subcommand.

- [ ] **Step 5: Write `src/cli.rs`**

```rust
//! Command-line surface.
//!
//! Every option is `Option<T>` and no option carries a clap `default_value`.
//! Defaults live in `config::Defaults` so that "unset" stays distinguishable
//! from "explicitly set to the default", which the template layer needs.

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
    Text(TextArgs),
    /// Remove everything this application has drawn
    Clear,
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
    #[arg(long, global = true)]
    pub json: bool,

    /// Print the payload that would be sent, and send nothing
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Suppress warnings
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Print more detail
    #[arg(short, long, global = true)]
    pub verbose: bool,
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
    #[arg(long, value_enum)]
    pub font: Option<FontArg>,

    /// Colour: #RRGGBBAA, #RRGGBB, #RGB, 0x-prefixed, bare hex, or a name
    #[arg(long)]
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
    #[arg(long, value_enum)]
    pub align: Option<AlignArg>,

    #[arg(long, value_enum)]
    pub screen: Option<ScreenArg>,
}

#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Scrolling")]
pub struct ScrollArgs {
    /// Width of the label in pixels
    #[arg(long)]
    pub width: Option<u16>,

    /// Scroll rate in PIXELS PER MINUTE
    #[arg(long)]
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
    #[arg(long)]
    pub priority: Option<String>,

    /// Seconds the element stays on screen (0 = forever)
    #[arg(long, conflicts_with = "until")]
    pub timeout: Option<u32>,

    /// Hide the element at this time: RFC 3339, or Unix seconds
    #[arg(long)]
    pub until: Option<String>,

    /// Blink the status LED this colour
    #[arg(long)]
    pub led: Option<String>,

    /// Element id, so repeat invocations update in place instead of accumulating
    #[arg(long)]
    pub id: Option<String>,

    /// Compose onto what is already on screen instead of replacing it
    #[arg(long)]
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
```

- [ ] **Step 6: Write `src/main.rs`**

```rust
mod cli;

use clap::Parser;

fn main() {
    let _cli = cli::Cli::parse();
    // Commands are wired up in later tasks.
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --test cli_surface`
Expected: PASS, 5 tests.

Also check the help reads well: `cargo run -- text --help`. The four `next_help_heading` groups should each appear as their own section.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore src/main.rs src/cli.rs tests/common/mod.rs tests/cli_surface.rs
git commit -m "feat: crate skeleton and clap command surface"
```

---

## Task 2: Lenient colour parsing

`busylib::types::color::Color::parse` is strict — it requires a leading `#` and exactly eight hex digits, with no `0x` form, no shorthand, and no names. The CLI does its own parsing and constructs `Color::rgba`.

**Files:**
- Create: `src/color.rs`
- Create: `src/device.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `color::parse(input: &str) -> Result<device::Color, String>`; `device::Color` (a re-export of `busylib::types::color::Color`).

- [ ] **Step 1: Write the failing test**

Append to `src/color.rs` (create the file with just this for now):

```rust
#[cfg(test)]
mod tests {
    use super::parse;

    fn hex(input: &str) -> String {
        parse(input).expect("should parse").to_string()
    }

    #[test]
    fn accepted_forms_all_reach_rrggbbaa() {
        assert_eq!(hex("#FF0000FF"), "#FF0000FF");
        assert_eq!(hex("#ff0000ff"), "#FF0000FF");
        assert_eq!(hex("0xFF0000FF"), "#FF0000FF");
        assert_eq!(hex("FF0000FF"), "#FF0000FF");
        assert_eq!(hex("#FF0000"), "#FF0000FF", "6 digits gain an opaque alpha");
        assert_eq!(hex("0xFF0000"), "#FF0000FF");
        assert_eq!(hex("FF0000"), "#FF0000FF");
        assert_eq!(hex("#F00"), "#FF0000FF", "3-digit shorthand doubles each nibble");
        assert_eq!(hex("#abc"), "#AABBCCFF");
        assert_eq!(hex("red"), "#FF0000FF");
        assert_eq!(hex("GREEN"), "#00FF00FF", "names are case-insensitive");
        assert_eq!(hex("blue"), "#0000FFFF");
        assert_eq!(hex("white"), "#FFFFFFFF");
        assert_eq!(hex("black"), "#000000FF");
        assert_eq!(hex("yellow"), "#FFFF00FF");
        assert_eq!(hex("orange"), "#FFA500FF");
        assert_eq!(hex("cyan"), "#00FFFFFF");
        assert_eq!(hex("magenta"), "#FF00FFFF");
    }

    #[test]
    fn alpha_is_preserved_when_given() {
        assert_eq!(hex("#FF000080"), "#FF000080");
    }

    #[test]
    fn rejected_forms_explain_themselves() {
        for bad in ["", "#", "#FF", "#FFFFF", "nope", "#GGGGGG", "0x"] {
            let error = parse(bad).expect_err("should be rejected");
            assert!(
                error.contains(bad) || bad.is_empty(),
                "error for {bad:?} should quote the input, got {error:?}"
            );
        }
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test color`
Expected: FAIL — `parse` is not defined.

- [ ] **Step 3: Write the implementation**

Create `src/device.rs` with only the re-exports needed so far:

```rust
//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

pub use busylib::types::color::Color;
```

Prepend to `src/color.rs`, above the `mod tests` block:

```rust
//! Lenient colour parsing.
//!
//! `busylib::types::color::Color::parse` requires exactly `#RRGGBBAA`. Users
//! type `red`, `0xff0000`, and `#f00`, so the CLI accepts those and constructs
//! the validated type itself.

use crate::device::Color;

/// Named colours the CLI understands, as `#RRGGBB`.
const NAMES: &[(&str, u32)] = &[
    ("red", 0xFF0000),
    ("green", 0x00FF00),
    ("blue", 0x0000FF),
    ("white", 0xFFFFFF),
    ("black", 0x000000),
    ("yellow", 0xFFFF00),
    ("orange", 0xFFA500),
    ("cyan", 0x00FFFF),
    ("magenta", 0xFF00FF),
];

pub fn parse(input: &str) -> Result<Color, String> {
    let trimmed = input.trim();

    let lower = trimmed.to_ascii_lowercase();
    if let Some((_, rgb)) = NAMES.iter().find(|(name, _)| *name == lower) {
        let [_, red, green, blue] = rgb.to_be_bytes();
        return Ok(Color::rgb(red, green, blue));
    }

    let hex = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("0x"))
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(input));
    }

    let nibble = |index: usize| -> u8 {
        u8::from_str_radix(&hex[index..index + 1], 16).expect("checked hex digit")
    };
    let byte = |index: usize| -> u8 {
        u8::from_str_radix(&hex[index..index + 2], 16).expect("checked hex digit")
    };

    match hex.len() {
        3 => Ok(Color::rgb(
            nibble(0) * 17,
            nibble(1) * 17,
            nibble(2) * 17,
        )),
        6 => Ok(Color::rgb(byte(0), byte(2), byte(4))),
        8 => Ok(Color::rgba(byte(0), byte(2), byte(4), byte(6))),
        _ => Err(invalid(input)),
    }
}

fn invalid(input: &str) -> String {
    format!(
        "invalid colour `{input}`: expected #RRGGBBAA, #RRGGBB, #RGB, a 0x-prefixed \
         or bare hex value, or one of red, green, blue, white, black, yellow, \
         orange, cyan, magenta"
    )
}
```

Wire both modules into `src/main.rs`:

```rust
mod cli;
mod color;
mod device;

use clap::Parser;

fn main() {
    let _cli = cli::Cli::parse();
    // Commands are wired up in later tasks.
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test color`
Expected: PASS, 3 tests.

Note: `3 => nibble * 17` is the standard shorthand expansion — `0xF * 17 == 0xFF`, `0xA * 17 == 0xAA`.

- [ ] **Step 5: Commit**

```bash
git add src/color.rs src/device.rs src/main.rs
git commit -m "feat: lenient colour parsing"
```

---

## Task 3: ASCII sanitization

The device's fonts are bitmap ASCII and `Text` is `^[\x20-\x7E]+$`. A commit subject, a chat paste, or anything that met a smart-quote substitution will contain U+2019 or an em-dash and be rejected wholesale. The CLI transliterates first and warns once.

**Files:**
- Create: `src/sanitize.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `sanitize::Sanitized { pub text: String, pub changed: bool }` and `sanitize::to_ascii(input: &str) -> Sanitized`.

- [ ] **Step 1: Write the failing test**

Create `src/sanitize.rs` containing only:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test sanitize`
Expected: FAIL — `to_ascii` is not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `src/sanitize.rs`:

```rust
//! Transliteration of common Unicode into the printable ASCII the device accepts.
//!
//! `Text` is `^[\x20-\x7E]+$` because the fonts are bitmap ASCII. A build
//! notification that fails because of a smart quote is a terrible experience,
//! so the CLI fixes what it can and warns once about what it changed.

/// The result of sanitizing, and whether anything was altered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub text: String,
    pub changed: bool,
}

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
```

Add `mod sanitize;` to `src/main.rs`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test sanitize`
Expected: PASS, 6 tests.

- [ ] **Step 5: Commit**

```bash
git add src/sanitize.rs src/main.rs
git commit -m "feat: transliterate Unicode text to printable ASCII"
```

---

## Task 4: Configuration precedence

Highest wins: CLI flags, then environment, then `~/.config/busy/config.toml`, then built-in `Defaults`. The resolver takes its inputs as values rather than reading the process environment, so tests are deterministic and can run in parallel.

**Files:**
- Create: `src/config.rs`
- Modify: `src/device.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `cli::{GlobalArgs, StyleArgs, PlacementArgs, FontArg, AlignArg, ScreenArg, PrefixArg}`, `color::parse`.
- Produces:
  - `config::Settings` with fields `addr: String`, `app: String`, `token: Option<String>`, `api_prefix: PrefixArg`, `http_timeout_ms: u64`, `font: device::Font`, `color: device::Color`, `screen: device::Screen`, `priority: u8`. **No `align` field** — align is resolved separately by `resolve_align`, because its flag lives on `PlacementArgs` rather than `StyleArgs` and because the API defines no default for it.
  - `config::FileConfig` (serde `Deserialize`, `Default`).
  - `config::Env { pub addr: Option<String>, pub token: Option<String>, pub app: Option<String> }` with `Env::from_process()`.
  - `config::resolve(global: &GlobalArgs, style: &StyleArgs, env: &Env, file: &FileConfig) -> Result<Settings, String>`.
  - `config::load_file() -> (FileConfig, Vec<String>)` — the config plus any warnings.
  - `config::parse_priority(input: &str) -> Result<u8, String>`.

- [ ] **Step 1: Extend `src/device.rs` with the remaining re-exports**

```rust
//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

pub use busylib::model::assets::{
    Align, DisplayElement, DisplayElements, Font, Lifetime, Screen, TextElement,
};
pub use busylib::types::app_name::AppName;
pub use busylib::types::color::Color;
pub use busylib::types::element_id::ElementId;
pub use busylib::types::priority::Priority;
pub use busylib::types::text::Text;
```

- [ ] **Step 2: Write the failing test**

Create `src/config.rs` containing only:

```rust
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
        // Two hashes, not one: this fixture contains `"#00ff00ff"`, and the
        // `"#` in it would otherwise terminate an `r#"…"#` literal early.
        let file = toml::from_str::<FileConfig>(
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
        let with_align = toml::from_str::<FileConfig>(
            r#"
            [defaults]
            align = "center"
            "#,
        )
        .unwrap();

        // No default at the API level: when nothing sets it, the field is
        // omitted from the payload and the device decides.
        assert_eq!(super::resolve_align(None, &empty), None);
        assert_eq!(super::resolve_align(None, &with_align), Some(Align::Center));
        assert_eq!(
            super::resolve_align(Some(AlignArg::TopLeft), &with_align),
            Some(Align::TopLeft),
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
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test config`
Expected: FAIL — `resolve`, `Settings`, `Env`, `FileConfig`, `parse_priority` are not defined.

- [ ] **Step 4: Write the implementation**

Prepend to `src/config.rs`:

```rust
//! Layered configuration.
//!
//! Precedence, highest first: CLI flags, environment, config file, built-in
//! `Defaults`. `resolve` takes every layer as a value rather than reading the
//! process environment, which keeps the tests deterministic under parallelism
//! and leaves one obvious seam for the template layer to slot into later.

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
    pub const HTTP_TIMEOUT_MS: u64 = 5000;
    pub const FONT: Font = Font::Large;
    pub const COLOR: Color = Color::rgba(0xff, 0xff, 0xff, 0xff);
    pub const SCREEN: Screen = Screen::Front;
    /// 95 beats an active BUSY work session at 90. The device's own default is
    /// 50, which loses exactly when the user is at their desk.
    pub const PRIORITY: u8 = 95;
    pub const ELEMENT_ID: &'static str = "message";
}

/// Values read from the environment. Constructed literally in tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub addr: Option<String>,
    pub token: Option<String>,
    pub app: Option<String>,
}

impl Env {
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

/// Everything a command needs after all four layers have been applied.
#[derive(Debug, Clone)]
pub struct Settings {
    pub addr: String,
    pub app: String,
    pub token: Option<String>,
    pub api_prefix: PrefixArg,
    pub http_timeout_ms: u64,
    pub font: Font,
    pub color: Color,
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
        api_prefix: global.api_prefix.unwrap_or(PrefixArg::Device),
        http_timeout_ms,
        font,
        color,
        screen,
        priority,
    })
}

/// Align is resolved separately because the flag lives on `PlacementArgs`
/// rather than `StyleArgs`, and because the API defines no default for it —
/// when nothing sets it, the field is omitted and the device decides.
pub fn resolve_align(flag: Option<AlignArg>, file: &FileConfig) -> Option<Align> {
    if let Some(align) = flag {
        return Some(align_from_arg(align));
    }
    file.defaults
        .align
        .as_deref()
        .and_then(|name| parse_enum::<Align>(name, "align").ok())
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
            "tiny", "small", "normal", "condensed", "bold", "large", "extra_large", "global",
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
```

Add `mod config;` to `src/main.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test config`
Expected: PASS, 8 tests.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/device.rs src/main.rs
git commit -m "feat: layered configuration with flag/env/file/default precedence"
```

---

## Task 5: Build the payload, and `--dry-run`

The highest-value test in the project. Because `--dry-run` serializes the same type `busylib` sends, its output *is* the wire payload, so flag/env/config precedence bugs surface here and almost nowhere else.

**Files:**
- Create: `src/cmd/mod.rs`
- Create: `src/cmd/text.rs`
- Create: `src/output.rs`
- Create: `src/error.rs`
- Modify: `src/main.rs`
- Test: `tests/payload.rs`

**Interfaces:**
- Consumes: `config::{Settings, resolve_align, parse_priority, align_from_arg, screen_from_arg}`, `sanitize::to_ascii`, `color::parse`, `device::{DisplayElement, DisplayElements, Font, Lifetime, TextElement, Text, Priority, ElementId, AppName}`.
- Produces:
  - `error::CliError` (enum, `thiserror::Error`) with variants `Usage(String)`, `Runtime(String)`, and `PriorityConflict { requested: u8, config: String }`, plus `CliError::exit_code`, `CliError::usage`, `CliError::runtime`, and `From<String>` (mapping to `Usage`, which is what lets `?` carry the `Result<_, String>` returned by `config::resolve`, `config::parse_priority`, and `color::parse`).
  - `cmd::text::build_payload(args: &cli::TextArgs, settings: &config::Settings, file: &config::FileConfig, message: &str) -> Result<device::DisplayElements, CliError>`.
  - `output::Emitter { json: bool, quiet: bool }` with `Emitter::warn(&self, message: &str)`, `Emitter::dry_run(&self, payload: &DisplayElements) -> Result<(), CliError>`, and `Emitter::success(&self, summary: &str, payload: &DisplayElements) -> Result<(), CliError>`.

- [ ] **Step 1: Write the failing test**

Create `tests/payload.rs`:

```rust
mod common;

use common::busy;

/// Run `busy`, require success, and hand back stdout — which under `--dry-run`
/// is the exact wire payload.
fn stdout(args: &[&str]) -> String {
    let output = busy().args(args).output().expect("should run");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be UTF-8")
}

#[test]
fn golden_payload_for_the_fully_specified_command() {
    let payload = stdout(&[
        "--dry-run",
        "text",
        "-x",
        "0",
        "-y",
        "8",
        "--align",
        "mid_left",
        "--font",
        "small",
        "--color",
        "0xFF0000FF",
        "Goodbye, World!",
    ]);
    insta::assert_snapshot!(payload);
}

#[test]
fn golden_payload_for_the_minimal_command() {
    insta::assert_snapshot!(stdout(&["--dry-run", "text", "Hello, World!"]));
}

#[test]
fn golden_payload_with_a_lifetime_and_an_led() {
    insta::assert_snapshot!(stdout(&[
        "--dry-run",
        "text",
        "--timeout",
        "30",
        "--led",
        "red",
        "--priority",
        "urgent",
        "deploy done",
    ]));
}

#[test]
fn smart_quotes_are_sanitized_into_the_payload() {
    let payload = stdout(&["--dry-run", "text", "don\u{2019}t \u{2014} really"]);
    assert!(
        payload.contains(r#""text": "don't - really""#),
        "got {payload}"
    );
}

#[test]
fn a_message_that_sanitizes_to_empty_is_a_clear_error() {
    let output = busy()
        .args(["--dry-run", "text", "\u{1f389}"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nothing printable"),
        "expected a clear message, got {stderr}"
    );
}

#[test]
fn timeout_and_until_conflict() {
    let output = busy()
        .args(["text", "--timeout", "30", "--until", "1900000000", "hi"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test payload`
Expected: FAIL — `--dry-run` prints nothing, so the snapshots are empty and the assertions fail.

- [ ] **Step 3: Write `src/error.rs`**

```rust
//! CLI-level errors.
//!
//! The priority-conflict message is the highest-value error string in the tool:
//! a 409 from `display/draw` means the bar is running something that outranks
//! this request, and a bare status code sends the user nowhere.

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The user asked for something impossible. Exits 2.
    #[error("{0}")]
    Usage(String),

    /// Something failed at run time. Exits 1.
    #[error("{0}")]
    Runtime(String),

    /// `display/draw` returned 409.
    #[error(
        "the bar is running an app at a higher priority than this request (priority {requested}).\n\
         An active BUSY or CUSTOM work session runs at 90; built-in apps run at 10.\n\
         Retry with `--priority 95`, or set `priority` under [defaults] in {config}."
    )]
    PriorityConflict { requested: u8, config: String },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::Runtime(_) | CliError::PriorityConflict { .. } => 1,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        CliError::Usage(message.into())
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        CliError::Runtime(message.into())
    }
}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        CliError::Usage(message)
    }
}
```

- [ ] **Step 4: Write `src/output.rs`**

```rust
//! Human, `--json`, and `--dry-run` emission.

use crate::device::DisplayElements;
use crate::error::CliError;

#[derive(Debug, Clone, Copy)]
pub struct Emitter {
    pub json: bool,
    pub quiet: bool,
}

impl Emitter {
    pub fn warn(&self, message: &str) {
        if !self.quiet {
            eprintln!("busy: warning: {message}");
        }
    }

    /// Print the exact bytes that would be sent, and nothing else.
    pub fn dry_run(&self, payload: &DisplayElements) -> Result<(), CliError> {
        let json = serde_json::to_string_pretty(payload)
            .map_err(|error| CliError::runtime(format!("could not serialize payload: {error}")))?;
        println!("{json}");
        Ok(())
    }

    pub fn success(&self, summary: &str, payload: &DisplayElements) -> Result<(), CliError> {
        if self.json {
            let body = serde_json::json!({
                "ok": true,
                "summary": summary,
                "payload": payload,
            });
            let json = serde_json::to_string_pretty(&body).map_err(|error| {
                CliError::runtime(format!("could not serialize output: {error}"))
            })?;
            println!("{json}");
        } else if !self.quiet {
            println!("{summary}");
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Write `src/cmd/mod.rs` and `src/cmd/text.rs`**

`src/cmd/mod.rs`:

```rust
pub mod text;
```

`src/cmd/text.rs`:

```rust
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
    let id = ElementId::new(id)
        .map_err(|error| CliError::usage(format!("invalid --id: {error}")))?;

    let mut builder = DisplayElement::builder(id)
        .map_err(|error| CliError::usage(error.to_string()))?
        .at(args.placement.x.unwrap_or(0), args.placement.y.unwrap_or(0));

    let screen = args
        .placement
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    builder = builder.screen(screen);

    if let Some(align) = config::resolve_align(args.placement.align, file) {
        builder = builder.align(align);
    }

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
```

- [ ] **Step 6: Wire it up in `src/main.rs`**

```rust
mod cli;
mod cmd;
mod color;
mod config;
mod device;
mod error;
mod output;
mod sanitize;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::CliError;
use crate::output::Emitter;

fn main() {
    let cli = Cli::parse();
    let emitter = Emitter {
        json: cli.global.json,
        quiet: cli.global.quiet,
    };

    if let Err(error) = run(&cli, emitter) {
        eprintln!("busy: {error}");
        std::process::exit(error.exit_code());
    }
}

fn run(cli: &Cli, emitter: Emitter) -> Result<(), CliError> {
    let (file, warnings) = config::load_file();
    for warning in &warnings {
        emitter.warn(warning);
    }

    let env = config::Env::from_process();

    match &cli.command {
        Command::Text(args) => {
            let settings = config::resolve(&cli.global, &args.style, &env, &file)?;
            // Task 11 replaces this with `input::read_message(&args.message)?`,
            // which adds `-` for stdin.
            let message = args.message.clone();

            let payload = cmd::text::build_payload(args, &settings, &file, &message)?;

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            // Sending arrives in Task 7.
            emitter.success("drawn", &payload)
        }
        Command::Clear => Err(CliError::runtime("`busy clear` arrives in Task 12")),
    }
}
```

- [ ] **Step 7: Run the tests and accept the snapshots**

Run: `cargo test --test payload`
Expected: the three `insta` snapshot tests fail as "new snapshot"; the other three pass.

Review and accept:

```bash
cargo insta review
```

The fully-specified payload must be exactly this — verified against `busylib` 0.0.11's own serialization:

```json
{
  "application_name": "busy",
  "priority": 95,
  "elements": [
    {
      "id": "message",
      "x": 0,
      "y": 8,
      "display": "front",
      "align": "mid_left",
      "type": "text",
      "text": "Goodbye, World!",
      "font": "small",
      "color": "#FF0000FF"
    }
  ]
}
```

Note `#FF0000FF` is uppercase — `Color`'s `Display` uses `{:02X}` — and that `timeout` appears as a bare sibling key of `id` when set, because `Lifetime` is `#[serde(untagged)]` and flattened.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --test payload`
Expected: PASS, 6 tests.

- [ ] **Step 9: Commit**

```bash
git add src/cmd src/output.rs src/error.rs src/main.rs tests/payload.rs tests/snapshots
git commit -m "feat: build draw payloads and print them with --dry-run"
```

---

## Task 6: Local bounds validation

The front display is 72×16 and the back is 160×80. An element placed outside those bounds renders nothing, with no error from the device. A local check costs nothing and saves a lot of confused staring.

**Files:**
- Create: `src/validate.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `device::{DisplayElements, Screen}`.
- Produces: `validate::bounds_warnings(payload: &DisplayElements) -> Vec<String>`.

- [ ] **Step 1: Write the failing test**

Create `src/validate.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::bounds_warnings;
    use crate::device::{
        AppName, DisplayElement, DisplayElements, Font, Screen, TextElement,
    };

    fn payload(x: i16, y: i16, screen: Screen) -> DisplayElements {
        let text = TextElement::new("hi", Font::Small).unwrap();
        let element = DisplayElement::builder("message")
            .unwrap()
            .at(x, y)
            .screen(screen)
            .text(text);
        DisplayElements::new(AppName::new("busy").unwrap())
            .unwrap()
            .element(element)
    }

    #[test]
    fn coordinates_inside_the_front_display_are_quiet() {
        assert!(bounds_warnings(&payload(0, 0, Screen::Front)).is_empty());
        assert!(bounds_warnings(&payload(71, 15, Screen::Front)).is_empty());
    }

    #[test]
    fn coordinates_past_the_front_display_warn() {
        let warnings = bounds_warnings(&payload(72, 0, Screen::Front));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("message"), "should name the element id");
        assert!(warnings[0].contains("72x16"), "should state the bounds");
    }

    #[test]
    fn negative_coordinates_warn() {
        assert_eq!(bounds_warnings(&payload(-1, 0, Screen::Front)).len(), 1);
        assert_eq!(bounds_warnings(&payload(0, -1, Screen::Front)).len(), 1);
    }

    #[test]
    fn the_back_display_is_larger() {
        assert!(bounds_warnings(&payload(100, 40, Screen::Back)).is_empty());
        assert_eq!(bounds_warnings(&payload(100, 40, Screen::Front)).len(), 1);
        assert_eq!(bounds_warnings(&payload(160, 0, Screen::Back)).len(), 1);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test validate`
Expected: FAIL — `bounds_warnings` is not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `src/validate.rs`:

```rust
//! Local checks that run before a request is sent.
//!
//! An element outside the display bounds renders nothing and the device reports
//! no error, so these are warnings the CLI raises itself. They never block a
//! send — the user may know something the CLI does not.

use crate::device::{DisplayElements, Screen};

/// Front display: 72x16 RGB. Back display: 160x80 in 16 greys.
const FRONT: (i16, i16) = (72, 16);
const BACK: (i16, i16) = (160, 80);

pub fn bounds_warnings(payload: &DisplayElements) -> Vec<String> {
    let mut warnings = Vec::new();

    for element in &payload.elements {
        let screen = element.display.unwrap_or(Screen::Front);
        let (width, height) = match screen {
            Screen::Front => FRONT,
            Screen::Back => BACK,
        };

        let x = element.x.unwrap_or(0);
        let y = element.y.unwrap_or(0);

        if x < 0 || x >= width || y < 0 || y >= height {
            warnings.push(format!(
                "element `{}` is anchored at ({x}, {y}), outside the {} display's \
                 {width}x{height} bounds; it will render nothing",
                element.id,
                match screen {
                    Screen::Front => "front",
                    Screen::Back => "back",
                }
            ));
        }
    }

    warnings
}
```

Add `mod validate;` to `src/main.rs`, and emit the warnings just before the dry-run branch in `run`:

```rust
            let payload = cmd::text::build_payload(args, &settings, &file, &message)?;

            for warning in validate::bounds_warnings(&payload) {
                emitter.warn(&warning);
            }

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test validate`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src/validate.rs src/main.rs
git commit -m "feat: warn about out-of-bounds element coordinates"
```

---

## Task 7: The device adapter, and actually drawing

**Files:**
- Modify: `src/device.rs`
- Modify: `src/main.rs`
- Test: `tests/device.rs`

**Interfaces:**
- Consumes: `config::Settings`, `error::CliError`, `device::DisplayElements`.
- Produces: `device::Device` with `Device::connect(settings: &config::Settings) -> Result<Device, CliError>`, `async fn draw(&self, payload: &DisplayElements) -> Result<(), CliError>`, and `async fn clear(&self) -> Result<(), CliError>`.

- [ ] **Step 1: Write the failing test**

Create `tests/device.rs`:

```rust
mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn a_draw_reaches_the_device() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "busy"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["text", "Hello, World!"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn the_cloud_prefix_is_selectable() {
    let server = MockServer::start().await;
    Mock::given(path("/busybar/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--api-prefix", "cloud", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn a_token_is_sent_as_a_bearer_header() {
    let server = MockServer::start().await;
    Mock::given(path("/api/display/draw"))
        .and(header("authorization", "Bearer 12345678"))
        .respond_with(ok())
        .expect(2) // the DELETE and the POST
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--token", "12345678", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn dry_run_sends_nothing() {
    let server = MockServer::start().await;
    // No mocks mounted: any request would 404 and fail the command.
    let output = busy_at(&server)
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"application_name\": \"busy\""));
}

#[tokio::test]
async fn an_unreachable_device_fails_with_exit_1() {
    let output = busy()
        .args(["--addr", "http://127.0.0.1:1", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("127.0.0.1:1"), "got {stderr}");
}
```

`#[tokio::test]` and `wiremock` need no new dependencies — Task 1's `Cargo.toml` already enables tokio's `rt-multi-thread` and `macros` features for exactly this.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test device`
Expected: FAIL — nothing is sent, so the `.expect(1)` mocks are unsatisfied.

- [ ] **Step 3: Extend `src/device.rs`**

Append below the existing re-exports:

```rust
use std::time::Duration;

use busylib::{ApiPrefix, Client, ClientBuilder, ReqwestHttpTransport};
use http::StatusCode;

use crate::cli::PrefixArg;
use crate::config::{self, Settings};
use crate::error::CliError;

/// A connected bar, plus the application name every request is scoped to.
pub struct Device {
    client: Client<ReqwestHttpTransport>,
    app: AppName,
}

impl Device {
    pub fn connect(settings: &Settings) -> Result<Self, CliError> {
        let prefix = match settings.api_prefix {
            PrefixArg::Device => ApiPrefix::Device,
            PrefixArg::Cloud => ApiPrefix::Cloud,
        };

        let mut builder = ClientBuilder::new(&settings.addr)
            .map_err(|error| CliError::usage(format!("invalid --addr: {error}")))?
            .api_prefix(prefix)
            .timeout(Duration::from_millis(settings.http_timeout_ms));

        if let Some(token) = &settings.token {
            builder = builder
                .token(token.as_str())
                .map_err(|error| CliError::usage(format!("invalid --token: {error}")))?;
        }

        let app = AppName::new(settings.app.clone())
            .map_err(|error| CliError::usage(format!("invalid --app: {error}")))?;

        Ok(Self {
            client: builder.build(ReqwestHttpTransport::new()),
            app,
        })
    }

    pub async fn draw(&self, payload: &DisplayElements) -> Result<(), CliError> {
        self.client
            .assets()
            .draw(payload)
            .await
            .map_err(|error| map_error(error, payload.priority.map(|p| p.percent())))
    }

    pub async fn clear(&self) -> Result<(), CliError> {
        self.client
            .assets()
            .clear(Some(self.app.clone()))
            .await
            .map_err(|error| map_error(error, None))
    }
}

/// Turn a `busylib` error into something a user can act on.
///
/// The 409 case is the important one: `display/draw` rejects a request whose
/// priority is below the running app's, and the raw status says nothing about
/// what to do next.
fn map_error(error: busylib::Error, requested_priority: Option<u8>) -> CliError {
    if error.is_status(StatusCode::CONFLICT) {
        return CliError::PriorityConflict {
            requested: requested_priority.unwrap_or(config::Defaults::PRIORITY),
            config: config::config_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "~/.config/busy/config.toml".to_owned()),
        };
    }

    if error.is_unauthorized() {
        return CliError::runtime(format!(
            "{error}\nThe bar requires an access key. Set BUSY_TOKEN, or `token` in the \
             config file. Configure the key on the device under Settings > HTTP API."
        ));
    }

    CliError::runtime(error.to_string())
}
```

`http` is already an explicit dependency from Task 1, so `use http::StatusCode;` resolves with no further change.

- [ ] **Step 4: Wire sending into `src/main.rs`**

Make `main` async and send the payload. Replace the whole file's `main`/`run` pair with:

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let emitter = Emitter {
        json: cli.global.json,
        quiet: cli.global.quiet,
    };

    if let Err(error) = run(&cli, emitter).await {
        eprintln!("busy: {error}");
        std::process::exit(error.exit_code());
    }
}

async fn run(cli: &Cli, emitter: Emitter) -> Result<(), CliError> {
    let (file, warnings) = config::load_file();
    for warning in &warnings {
        emitter.warn(warning);
    }

    let env = config::Env::from_process();

    match &cli.command {
        Command::Text(args) => {
            let settings = config::resolve(&cli.global, &args.style, &env, &file)?;
            // Task 11 replaces this with `input::read_message(&args.message)?`,
            // which adds `-` for stdin.
            let message = args.message.clone();

            let payload = cmd::text::build_payload(args, &settings, &file, &message)?;

            for warning in validate::bounds_warnings(&payload) {
                emitter.warn(&warning);
            }

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            let device = device::Device::connect(&settings)?;
            // Replace-by-default is Task 9; for now always clear first.
            device.clear().await?;
            device.draw(&payload).await?;

            emitter.success("drawn", &payload)
        }
        Command::Clear => Err(CliError::runtime("`busy clear` arrives in Task 12")),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test device`
Expected: PASS, 5 tests.

- [ ] **Step 6: Try it against the real bar**

```bash
cargo run -- text "Hello, World!"
cargo run -- --dry-run text "Hello, World!"
```

Expected: the bar shows the message; `--dry-run` prints the payload and touches nothing.

- [ ] **Step 7: Commit**

```bash
git add src/device.rs src/main.rs Cargo.toml Cargo.lock tests/device.rs
git commit -m "feat: send draw requests to the device"
```

---

## Task 8: Priority conflicts and error quality

`CliError::PriorityConflict` exists from Task 5 and `map_error` produces it from Task 7. This task proves it end to end and covers the other error paths.

**Files:**
- Test: `tests/errors.rs`
- Modify: `src/device.rs` (only if a test exposes a gap)

**Interfaces:**
- Consumes: everything from Task 7.
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

Create `tests/errors.rs`:

```rust
mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Let the clear succeed and the draw fail, which is the shape every error
/// case here needs.
async fn draw_responds(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_409_becomes_priority_guidance_not_a_status_code() {
    let server = MockServer::start().await;
    draw_responds(
        &server,
        409,
        serde_json::json!({"error": "Requested priority level is below that of currently active app."}),
    )
    .await;

    let output = busy_at(&server)
        .args(["text", "Build Failed!"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("work session"), "got {stderr}");
    assert!(stderr.contains("--priority 95"), "got {stderr}");
    assert!(stderr.contains("config.toml"), "got {stderr}");
    assert!(!stderr.contains("409"), "the raw status should not lead: {stderr}");
}

#[tokio::test]
async fn the_reported_priority_is_the_one_that_was_requested() {
    let server = MockServer::start().await;
    draw_responds(&server, 409, serde_json::json!({"error": "nope"})).await;

    let output = busy_at(&server)
        .args(["text", "--priority", "low", "hi"])
        .output()
        .expect("should run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("priority 10"), "got {stderr}");
}

#[tokio::test]
async fn a_401_explains_the_access_key() {
    let server = MockServer::start().await;
    draw_responds(&server, 401, serde_json::json!({"error": "Unauthorized"})).await;

    let output = busy_at(&server).args(["text", "hi"]).output().expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BUSY_TOKEN"), "got {stderr}");
}

#[tokio::test]
async fn a_400_surfaces_the_device_message() {
    let server = MockServer::start().await;
    draw_responds(
        &server,
        400,
        serde_json::json!({"error": "Failed to decode image /ext/user_assets/busy/nope.png."}),
    )
    .await;

    let output = busy_at(&server).args(["text", "hi"]).output().expect("should run");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to decode image"), "got {stderr}");
}

#[test]
fn a_bad_address_is_a_usage_error() {
    let output = busy()
        .args(["--addr", "ftp://nope", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--addr"));
}
```

- [ ] **Step 2: Run the tests to verify the state of play**

Run: `cargo test --test errors`
Expected: mostly PASS already, because Task 7 built the mapping. Any failure points at a real gap — fix it in `src/device.rs`'s `map_error` rather than weakening the test.

The likely genuine failure is `the_reported_priority_is_the_one_that_was_requested`: `map_error` reads the priority off the payload, so confirm `payload.priority` is populated (it always is — `build_payload` sets it unconditionally).

- [ ] **Step 3: Fix any gap, then re-run**

Run: `cargo test --test errors`
Expected: PASS, 5 tests.

- [ ] **Step 4: Commit**

```bash
git add tests/errors.rs src/device.rs
git commit -m "test: cover priority conflicts and device error mapping"
```

---

## Task 9: `--id`, `--keep`, and replace-by-default

`POST display/draw` upserts elements by `id` and never removes anything, so without an explicit clear a template's icon survives the next plain text draw. Default to replace; `--keep` composes.

**Files:**
- Modify: `src/main.rs`
- Test: `tests/replace.rs`

**Interfaces:**
- Consumes: `device::Device::{clear, draw}`.
- Produces: no new public API; changes `run`'s behaviour so the `DELETE` is conditional on `--keep`.

- [ ] **Step 1: Write the failing test**

Create `tests/replace.rs`:

```rust
mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn a_plain_draw_clears_first() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server).args(["text", "hi"]).output().expect("should run");
    assert!(output.status.success());
    // MockServer verifies the .expect() counts when it drops.
}

#[tokio::test]
async fn keep_skips_the_clear() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["text", "--keep", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[test]
fn the_default_element_id_is_message() {
    let output = busy()
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""id": "message""#));
}

#[test]
fn the_element_id_is_overridable() {
    let output = busy()
        .args(["--dry-run", "text", "--id", "status-line", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""id": "status-line""#));
}

#[test]
fn an_element_id_with_a_space_is_rejected() {
    let output = busy()
        .args(["--dry-run", "text", "--id", "status line", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--id"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test replace`
Expected: FAIL — `keep_skips_the_clear`, because Task 7 clears unconditionally.

- [ ] **Step 3: Make the clear conditional**

In `src/main.rs`, replace the two send lines:

```rust
            let device = device::Device::connect(&settings)?;

            // POST display/draw upserts by id and never removes, so a previous
            // multi-element draw would leave its other elements on screen.
            // Replacing by default makes every invocation independent of history;
            // --keep is for scripts that update one element of a live layout.
            if !args.delivery.keep {
                device.clear().await?;
            }
            device.draw(&payload).await?;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test replace`
Expected: PASS, 5 tests.

- [ ] **Step 5: Check for flicker on the real device**

```bash
cargo run -- text "one"
cargo run -- text "two"
```

Watch the bar. If the `DELETE`-then-`POST` visibly blanks the display between the two, note it in `docs/busy-cli-architecture.md` §5.3 — the fallback (tracking the last-written id set in a state file) is specified there. **Do not build the fallback unless the flicker is real.**

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/replace.rs
git commit -m "feat: replace by default, compose with --keep"
```

---

## Task 10: `--json` output and quiet mode

**Files:**
- Modify: `src/output.rs`
- Modify: `src/main.rs`
- Test: `tests/output.rs`

**Interfaces:**
- Consumes: `output::Emitter`.
- Produces: `Emitter::error_json(&self, error: &CliError)`; `Emitter` gains no new fields.

- [ ] **Step 1: Write the failing test**

Create `tests/output.rs`:

```rust
mod common;

use common::{busy, busy_at, ok};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_ok(server: &MockServer) {
    Mock::given(path("/api/display/draw"))
        .respond_with(ok())
        .mount(server)
        .await;
}

#[tokio::test]
async fn json_success_is_parseable_and_carries_the_payload() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let output = busy_at(&server)
        .args(["--json", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(value["ok"], serde_json::json!(true));
    assert_eq!(value["payload"]["application_name"], serde_json::json!("busy"));
    assert_eq!(value["payload"]["elements"][0]["text"], serde_json::json!("hi"));
}

#[tokio::test]
async fn json_failure_is_parseable_and_goes_to_stderr() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .respond_with(ok())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/display/draw"))
        .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({"error": "nope"})))
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--json", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(1));

    let value: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(value["ok"], serde_json::json!(false));
    assert!(value["error"].as_str().unwrap().contains("work session"));
}

#[tokio::test]
async fn quiet_suppresses_the_success_line() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let output = busy_at(&server)
        .args(["--quiet", "text", "hi"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "got {:?}", output.stdout);
}

#[tokio::test]
async fn quiet_suppresses_bounds_warnings() {
    let server = MockServer::start().await;
    mount_ok(&server).await;

    let noisy = busy_at(&server)
        .args(["text", "-x", "500", "hi"])
        .output()
        .expect("should run");
    assert!(String::from_utf8_lossy(&noisy.stderr).contains("outside"));

    let quiet = busy_at(&server)
        .args(["--quiet", "text", "-x", "500", "hi"])
        .output()
        .expect("should run");
    assert!(quiet.stderr.is_empty(), "got {:?}", quiet.stderr);
}

#[test]
fn dry_run_output_is_unaffected_by_json() {
    // --dry-run already emits the exact wire payload, so --json must not wrap it.
    let plain = busy()
        .args(["--dry-run", "text", "hi"])
        .output()
        .expect("should run");
    let jsonic = busy()
        .args(["--dry-run", "--json", "text", "hi"])
        .output()
        .expect("should run");
    assert_eq!(plain.stdout, jsonic.stdout);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test output`
Expected: FAIL — `json_failure_is_parseable_and_goes_to_stderr`, because `main` prints a plain `busy: {error}`.

- [ ] **Step 3: Add JSON error emission**

Append to `src/output.rs`:

```rust
impl Emitter {
    /// Report a failure. Under `--json` this writes a parseable object to
    /// stderr so a wrapper script can branch on it without scraping prose.
    pub fn failure(&self, error: &CliError) {
        if self.json {
            let body = serde_json::json!({
                "ok": false,
                "error": error.to_string(),
            });
            match serde_json::to_string_pretty(&body) {
                Ok(json) => eprintln!("{json}"),
                Err(_) => eprintln!("busy: {error}"),
            }
        } else {
            eprintln!("busy: {error}");
        }
    }
}
```

In `src/main.rs`, route the failure through the emitter:

```rust
    if let Err(error) = run(&cli, emitter).await {
        emitter.failure(&error);
        std::process::exit(error.exit_code());
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test output`
Expected: PASS, 5 tests.

- [ ] **Step 5: Commit**

```bash
git add src/output.rs src/main.rs tests/output.rs
git commit -m "feat: machine-readable --json output for success and failure"
```

---

## Task 11: Reading the message from stdin

In CI the message is usually already in a pipe. `-` is the conventional sentinel and upstream `busybar assets draw --file -` already uses it.

**Files:**
- Create: `src/input.rs`
- Modify: `src/main.rs`
- Test: `tests/stdin.rs`

**Interfaces:**
- Consumes: `error::CliError`.
- Produces: `input::read_message(argument: &str) -> Result<String, CliError>`.

- [ ] **Step 1: Write the failing test**

Create `tests/stdin.rs`:

```rust
mod common;

use common::busy;

#[test]
fn a_dash_reads_the_message_from_stdin() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("Build failed\n")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(r#""text": "Build failed""#),
        "trailing newline should be trimmed"
    );
}

#[test]
fn only_the_final_newline_is_trimmed() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("a  b\n")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "a  b""#));
}

#[test]
fn empty_stdin_is_a_clear_error() {
    let output = busy()
        .args(["--dry-run", "text", "-"])
        .write_stdin("")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("stdin"));
}

#[test]
fn a_literal_dash_message_is_still_reachable() {
    // `--` terminates option parsing; the value after it is a literal.
    let output = busy()
        .args(["--dry-run", "text", "--", "-3 tests failing"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""text": "-3 tests failing""#));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test stdin`
Expected: FAIL — `-` is currently taken as the literal message `-`.

- [ ] **Step 3: Write `src/input.rs`**

```rust
//! Where a message comes from.

use std::io::Read as _;

use crate::error::CliError;

/// Resolve the message argument. `-` means stdin, which is how CI usually has
/// the text already: `git log -1 --format=%s | busy text -`.
///
/// A literal `-` as the message is still reachable with `busy text -- -`.
pub fn read_message(argument: &str) -> Result<String, CliError> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }

    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer).map_err(|error| {
        CliError::usage(format!("could not read the message from stdin: {error}"))
    })?;

    // Strip only the line ending a pipe adds, never meaningful whitespace.
    let mut message = buffer.as_str();
    if let Some(trimmed) = message.strip_suffix('\n') {
        message = trimmed;
    }
    if let Some(trimmed) = message.strip_suffix('\r') {
        message = trimmed;
    }

    if message.is_empty() {
        return Err(CliError::usage(
            "stdin was empty; `busy text -` expects a message on stdin",
        ));
    }

    Ok(message.to_owned())
}
```

- [ ] **Step 4: Use it in `src/main.rs`**

Add `mod input;`, and replace the two-line message resolution from Task 5 with:

```rust
            let message = input::read_message(&args.message)?;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test stdin`
Expected: PASS, 4 tests.

- [ ] **Step 6: Commit**

```bash
git add src/input.rs src/main.rs tests/stdin.rs
git commit -m "feat: read the message from stdin with `-`"
```

---

## Task 12: `busy clear`, and the release pass

**Files:**
- Create: `src/cmd/clear.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`
- Create: `README.md`
- Test: `tests/clear.rs`

**Interfaces:**
- Consumes: `device::Device::clear`, `output::Emitter`.
- Produces: `async cmd::clear::run(device: &device::Device, app: &str, emitter: output::Emitter, dry_run: bool) -> Result<(), CliError>`.

- [ ] **Step 1: Write the failing test**

Create `tests/clear.rs`:

```rust
mod common;

use common::{busy_at, ok};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn clear_deletes_this_apps_elements() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "busy"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server).arg("clear").output().expect("should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn clear_is_scoped_to_the_selected_app() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/display/draw"))
        .and(query_param("application_name", "ci"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = busy_at(&server)
        .args(["--app", "ci", "clear"])
        .output()
        .expect("should run");
    assert!(output.status.success());
}

#[tokio::test]
async fn clear_honours_dry_run() {
    let server = MockServer::start().await;
    // No mocks: a request would 404 and fail.
    let output = busy_at(&server)
        .args(["--dry-run", "clear"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("clear"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test clear`
Expected: FAIL — `busy clear` still returns the Task 12 placeholder error.

- [ ] **Step 3: Write `src/cmd/clear.rs`**

```rust
//! `busy clear` — remove everything this application has drawn.
//!
//! Scoped to `application_name`, so it never disturbs another app's elements.

use crate::device::Device;
use crate::error::CliError;
use crate::output::Emitter;

pub async fn run(device: &Device, app: &str, emitter: Emitter, dry_run: bool) -> Result<(), CliError> {
    if dry_run {
        println!("would clear all elements drawn by `{app}`");
        return Ok(());
    }

    device.clear().await?;

    if emitter.json {
        println!("{{\n  \"ok\": true,\n  \"summary\": \"cleared\"\n}}");
    } else if !emitter.quiet {
        println!("cleared");
    }

    Ok(())
}
```

Add `pub mod clear;` to `src/cmd/mod.rs`, and replace the placeholder arm in `src/main.rs`:

```rust
        Command::Clear => {
            let settings = config::resolve(
                &cli.global,
                &cli::StyleArgs::default(),
                &env,
                &file,
            )?;
            let device = device::Device::connect(&settings)?;
            cmd::clear::run(&device, &settings.app, emitter, cli.global.dry_run).await
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test clear`
Expected: PASS, 3 tests.

- [ ] **Step 5: Run everything, and lint**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all green. Fix anything clippy flags; do not add `#[allow]` without a comment explaining why.

- [ ] **Step 6: Write `README.md`**

```markdown
# busy

An ergonomic CLI for the [BUSY Bar](https://busy.app).

```sh
busy text "Hello, World!"
busy text -x 0 -y 8 --align mid_left --font small --color red "Goodbye!"
busy text --timeout 30 --priority urgent "deploy done"
git log -1 --format=%s | busy text -
busy clear
```

## Install

```sh
cargo install --path .
```

## Configuration

Highest precedence wins: CLI flags, then environment (`BUSY_ADDR`, `BUSY_TOKEN`,
`BUSY_APP`), then `~/.config/busy/config.toml`, then built-in defaults.

```toml
addr = "http://10.0.4.20"
app  = "busy"

[defaults]
font     = "large"
align    = "center"
color    = "#ffffffff"
priority = 95
```

Keep the access key out of `argv` — prefer `BUSY_TOKEN` or the config file so it
stays out of your shell history and out of `ps`.

## Notes

- **Priority.** A draw is accepted only when its priority is at least the running
  app's. Built-in apps run at 10; an active BUSY work session runs at 90. The
  CLI defaults to 95 so a deliberate notification wins. `--priority` also accepts
  `low`, `normal`, `high`, `urgent` (10, 50, 95, 100).
- **Replace by default.** `POST display/draw` upserts by element id and never
  removes, so `busy` clears its own elements before drawing. Pass `--keep` to
  compose onto what is already on screen.
- **ASCII only.** The bar's fonts are bitmap ASCII. Smart quotes, dashes, and
  ellipses are transliterated automatically and a warning is printed; anything
  else is dropped.
- `--dry-run` prints the exact JSON that would be sent and contacts nothing.

## Prior art

[`busybar-rust`](https://github.com/foresterre/busybar-rust) provides `busylib`,
which this tool is built on, and a `busybar` CLI that mirrors the API 1:1. For
frame capture and screen mirroring, use that.
```

- [ ] **Step 7: Verify against the real bar**

```bash
cargo run -- text "Hello, World!"
cargo run -- text -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF "Goodbye, World!"
cargo run -- text --timeout 5 "gone in five"
cargo run -- clear
```

Expected: each renders as described; the third disappears after five seconds; the fourth blanks the bar.

Start a BUSY work session on the device and run `cargo run -- text "during a session"`. Expected: it draws, because the default priority of 95 beats the session's 90. Then `cargo run -- text --priority low "should fail"` and confirm the guidance message appears rather than a bare 409.

- [ ] **Step 8: Commit**

```bash
git add src/cmd/clear.rs src/cmd/mod.rs src/main.rs README.md tests/clear.rs
git commit -m "feat: busy clear, plus README and release checks"
```

---

## Definition of done

Phases 1–2 of the architecture doc are complete when all of the following hold:

- `busy text "Hello, World!"` lights up a real bar.
- `busy --dry-run text …` prints the exact wire payload and contacts nothing.
- `busy text -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF "Goodbye, World!"` renders correctly and matches its golden snapshot.
- A message appears during an active focus session, and a deliberately low priority produces the §5.1 guidance rather than a bare 409.
- `busy text "don't — really"` renders, having warned once about the substitution.
- `busy text "🎉"` fails with a clear message rather than a regex error.
- `git log -1 --format=%s | busy text -` works.
- `busy clear` blanks only this app's elements.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` are green.

## Deferred to later plans

- **Phase 3:** `busy asset upload|list|delete`, `busy draw` with stock/asset resolution, image re-encoding, device-authoritative asset sync (spec §5.5).
- **Phase 4:** the template layer — TOML over `DisplayElements`, minijinja with strict undefined behaviour, `undeclared_variables` for required-variable errors, template resolution added to `busy draw`, `--as`, the inert-flag check (spec §3.3), and `--id` becoming an error for templates (spec §3.4).
- **Phase 5:** `busy status`, `clap_complete` completions, a `--version` that also reports the device's API version.
- **Upstream:** the two missing `#[cfg(feature = "ws")]` attributes in `busylib` 0.0.11's `src/api/mod.rs`.
