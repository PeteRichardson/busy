# Phase 4a — Templates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `busy template init` writes example templates to `~/.config/busy/templates/`, and `busy draw error "Build failed"` renders one and puts it on the bar.

**Architecture:** A template is a directory holding `template.toml`. minijinja renders it — every substitution auto-escaped for TOML — into a `TemplateFile`, which supplies the envelope `DisplayElements` needs. `src/template/` splits into four focused files, with `render.rs` the only module naming minijinja, matching the containment rule `device.rs` and `image.rs` already follow. A new `overrides.rs` holds one applicability table shared by templates and `--file`, replacing four hand-written rejection chains.

**Tech Stack:** Rust 2024, `busylib` 0.0.11, `minijinja` 2.23, `include_dir` 0.7, `clap` 4, `toml`, `insta` + `wiremock` + `assert_cmd` for tests.

**Source documents:**
- `docs/specs/2026-08-11-phase-4a-templates-design.md` — the authority for this phase
- `docs/specs/2026-08-09-busy-cli-ux-design.md` — the command surface (§3 draw, §4 templates)
- `docs/busy-cli-architecture.md` — device behaviour and the original template sketch

## Global Constraints

- **Only `src/device.rs` may write `use busylib::…`. Only `src/image.rs` may write `use image::…`. Only `src/template/render.rs` may write `use minijinja::…`.** Everything else imports from those three.
- **Every CLI option is `Option<T>`. Never use clap's `default_value`.** Defaults live in exactly one place, `config::Defaults`. Boolean presence flags are plain `bool`.
- Exit codes: 0 success, 1 runtime failure, 2 usage error. `CliError::Usage` → 2; `Runtime` and `PriorityConflict` → 1.
- `busylib = "=0.0.11"` pinned, default features. The `reqwest`-only combination does not compile in 0.0.11.
- **A flag that parses but does nothing is a defect.** Anything inapplicable is a hard error, or is not offered at all.
- **Never modify a `.snap` file to make a test pass.** Existing golden snapshots are verified against real hardware.
- The device is at `http://10.0.4.20`, no token. Leave the display cleared after any real-device check.

### Prerequisites — land these before Task 1

- **Issue #12** — `busy draw --help` advertises `--until` and then exits 2. Task 7 rewrites that region; fixing it first avoids doing the work twice.
- **Issues #9 + #10** — the error-kind contract. `impl From<String> for CliError` maps every string error to `Usage`. This phase adds a dozen error paths; write them against the fixed contract.

### Verified against the real crates — do not re-derive

Checked by compiling and running before this plan was written:

- `env.set_undefined_behavior(UndefinedBehavior::Strict)` makes a missing variable fail with `err.kind() == ErrorKind::UndefinedError`.
- `env.set_auto_escape_callback(|_| AutoEscape::Custom("toml"))` plus `env.set_formatter(..)` gives a custom escaper. **The formatter MUST check `value.is_safe()`** — without it, `| safe` is silently ignored. This was caught by running it.
- `template.undeclared_variables(false)` returns `HashSet<String>`.
- **Escaping verified end to end:** `text = "{{ message }}"` with `He said "hi"\nbye` renders `text = "He said \"hi\"\nbye"`, which parses as valid TOML; `x = {{ pos }}` with `4` renders `x = 4`, untouched.
- **`toml::from_str::<DisplayElements>` FAILS** with `missing field 'application_name'`. A template must not carry that field. Hence `TemplateFile` (Task 3).
- `DisplayElements` has **no** `deny_unknown_fields`, so a `description` key parses and is silently discarded.
- A `TemplateFile` carrying a `rectangle` element parses and serializes to the expected wire JSON with no code naming a rectangle — the inherited "free element kinds" claim holds.
- `include_dir!("$CARGO_MANIFEST_DIR/templates")`: `Dir::dirs()`, `Dir::files()`, `File::contents() -> &[u8]`. **`File::path()` and `Dir::path()` are relative to the embed root**, so the lookup is `d.get_file(d.path().join("template.toml"))` — `d.get_file("template.toml")` returns `None`.
- `ElementKind::{Text, Image, Animation, Countdown, Rectangle}`; `ImageElement { source: ImageSource, opacity }`; `ImageSource::{Asset { path }, Stock { stock_path }}`.

### Existing code this phase builds on

- `Emitter`: `warn`, `warn_always`, `dry_run(&DisplayElements)`, `success(&str, Option<&DisplayElements>)`, `success_list(&str, &[(String, Option<u64>, bool)])`.
- `CliError::usage(impl Into<String>)`, `CliError::runtime(..)`.
- `config::Defaults`, `config::config_path()`, `resolve_align`, `parse_priority`, `screen_from_arg`.
- `validate::bounds_warnings(&DisplayElements) -> Vec<String>`.
- `Device::list_assets() -> Result<Vec<StorageListElement>, CliError>`; `StorageListElement::{name, size, is_dir}`.
- `tests/common/mod.rs` provides `busy()`, `busy_at(&MockServer)`, `ok()`.

---

## File Structure

```
templates/                    # NEW — embedded at compile time
├── error/template.toml
└── ok/template.toml
src/
├── template/
│   ├── mod.rs                # NEW — TemplateFile, Template, load, bind_variables
│   ├── discover.rs           # NEW — root, name validation, list, suggest
│   ├── render.rs             # NEW — ONLY module naming minijinja
│   └── validate.rs           # NEW — offline checks
├── overrides.rs              # NEW — Kind + the applicability table
├── cmd/template.rs           # NEW — init | list | show | validate | run
├── cmd/draw.rs               # MODIFY — resolution rule 2; drop ad-hoc rejections
├── cli.rs                    # MODIFY — TemplateCmd, --var, --template-dir
├── device.rs                 # MODIFY — re-export ImageSource
└── main.rs                   # MODIFY — wire the Template command
tests/
├── template.rs               # NEW
├── examples.rs               # NEW
└── overrides.rs              # NEW
```

---

## Task 1: Template discovery

**Files:**
- Create: `src/template/mod.rs` (stub), `src/template/discover.rs`
- Modify: `src/main.rs`, `src/cli.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `crate::error::CliError`.
- Produces:
  - `template::discover::root(flag: Option<&Path>) -> Option<PathBuf>`
  - `template::discover::validate_name(name: &str) -> Result<(), CliError>`
  - `template::discover::list(root: &Path) -> Vec<String>`
  - `template::discover::suggest(name: &str, candidates: &[String]) -> Option<String>`
  - `cli::GlobalArgs::template_dir: Option<PathBuf>`

- [ ] **Step 1: Add `--template-dir` to `src/cli.rs`**

In `GlobalArgs`, after `http_timeout`:

```rust
    /// Directory holding template directories (default ~/.config/busy/templates)
    #[arg(long, global = true)]
    pub template_dir: Option<PathBuf>,
```

Long-only, matching the other connection/config globals: it is typed rarely and a global short is reserved across every subcommand.

- [ ] **Step 2: Write the failing tests**

Create `src/template/discover.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::{list, suggest, validate_name};

    #[test]
    fn a_name_with_a_path_separator_is_rejected() {
        // The name becomes a directory under the root, so `/` or `..` would
        // let a template name escape it. Rejected before any filesystem access.
        for bad in ["../etc", "a/b", "/abs", ".."] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn ordinary_names_are_accepted() {
        for good in ["error", "ok", "build-status", "deploy_v2", "a.b"] {
            assert!(validate_name(good).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn listing_skips_directories_without_a_template_toml() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("error")).unwrap();
        std::fs::write(dir.join("error/template.toml"), "elements = []").unwrap();
        std::fs::create_dir_all(dir.join("not-a-template")).unwrap();
        std::fs::write(dir.join("loose.txt"), "x").unwrap();

        assert_eq!(list(&dir), vec!["error".to_owned()]);
    }

    #[test]
    fn listing_a_missing_root_is_empty_not_an_error() {
        // A missing root means "no templates", exactly as a missing config
        // file means "no config".
        assert!(list(std::path::Path::new("/nonexistent/templates")).is_empty());
    }

    #[test]
    fn the_flag_wins_over_the_default_root() {
        let chosen = super::root(Some(std::path::Path::new("/tmp/elsewhere")));
        assert_eq!(chosen, Some(std::path::PathBuf::from("/tmp/elsewhere")));
    }

    #[test]
    fn suggest_finds_a_near_match_and_ignores_a_distant_one() {
        let names = vec!["error".to_owned(), "ok".to_owned()];
        assert_eq!(suggest("eror", &names), Some("error".to_owned()));
        assert_eq!(suggest("wildly-different", &names), None);
    }

    /// A unique temp directory for one test. `std::env::temp_dir()` is shared,
    /// and the suite runs in parallel.
    fn tempdir() -> std::path::PathBuf {
        let unique = format!(
            "busy-discover-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test discover`
Expected: FAIL — `list`, `suggest`, `validate_name` are not defined.

- [ ] **Step 4: Implement**

Prepend to `src/template/discover.rs`:

```rust
//! Finding templates on disk.
//!
//! A template is a directory containing `template.toml`. A directory without
//! one is not a template and is skipped silently rather than reported — the
//! root is a user directory, not a manifest.

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// The template root, highest precedence first: `--template-dir`, then
/// `~/.config/busy/templates`.
///
/// `None` means the platform gave us no config directory at all. A root that
/// simply does not exist yet is `Some` — `list` reports it as empty and
/// `template init` creates it.
pub fn root(flag: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = flag {
        return Some(path.to_path_buf());
    }
    let strategy = etcetera::choose_base_strategy().ok()?;
    use etcetera::BaseStrategy as _;
    Some(strategy.config_dir().join("busy").join("templates"))
}

/// Reject anything that is not a single path component.
///
/// The name is joined onto the root, so `..` or a separator would let a
/// template name reach outside it. Same charset as `AssetName`.
pub fn validate_name(name: &str) -> Result<(), CliError> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        return Ok(());
    }
    Err(CliError::usage(format!(
        "`{name}` is not a usable template name: use only letters, digits, dot, \
         underscore, or hyphen."
    )))
}

/// Every template name under `root`, sorted. Never fails: an unreadable root
/// is indistinguishable from an empty one for this purpose.
pub fn list(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().join("template.toml").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// The closest candidate to `name`, when one is close enough to be worth
/// suggesting. Powers did-you-mean on a misresolved draw.
pub fn suggest(name: &str, candidates: &[String]) -> Option<String> {
    let threshold = 2.max(name.len() / 3);
    candidates
        .iter()
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|(d, _)| *d <= threshold)
        .min_by_key(|(d, _)| *d)
        .map(|(_, candidate)| candidate.clone())
}

/// Levenshtein distance, two-row variant. A `strsim` dependency for one call
/// site is not worth the tree.
fn distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current = vec![0usize; b_chars.len() + 1];

    for (i, a_char) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b_chars.iter().enumerate() {
            let cost = usize::from(a_char != *b_char);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b_chars.len()]
}
```

Create `src/template/mod.rs` with just `pub mod discover;` for now, and add `mod template;` to `src/main.rs` alphabetically — it sorts between `sanitize` and `validate`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test discover`
Expected: PASS, 6 tests.

Then `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. All clean.

- [ ] **Step 6: Commit**

```bash
git add src/template/ src/main.rs src/cli.rs
git commit -m "feat: template discovery, name validation, and did-you-mean"
```

---

## Task 2: Rendering — the minijinja boundary

**Files:**
- Create: `src/template/render.rs`
- Modify: `src/template/mod.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `crate::error::CliError`.
- Produces:
  - `template::render::analyse(name: &str, source: &str) -> Result<Vec<String>, CliError>` — the template's required variables, sorted.
  - `template::render::render(name: &str, source: &str, vars: &BTreeMap<String, String>) -> Result<String, CliError>`

- [ ] **Step 1: Add the dependency**

```bash
cargo add minijinja
```

Expect `minijinja = "2.23.0"`. Default features are correct here — `undeclared_variables` and the custom auto-escape both live in the default set, verified before this plan was written.

- [ ] **Step 2: Write the failing tests**

Create `src/template/render.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::{analyse, render};
    use std::collections::BTreeMap;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_quote_in_a_value_cannot_break_the_document() {
        // The whole reason auto-escaping exists: `message` routinely arrives
        // from `git log -1 --format=%s`, and a commit subject may contain
        // anything. Verified against minijinja before this plan was written.
        let out = render(
            "t",
            r#"text = "{{ message }}""#,
            &vars(&[("message", "He said \"hi\"")]),
        )
        .expect("should render");
        assert_eq!(out, r#"text = "He said \"hi\"""#);
        toml::from_str::<toml::Value>(&out).expect("escaped output must be valid TOML");
    }

    #[test]
    fn backslashes_and_control_characters_are_escaped() {
        let out = render(
            "t",
            r#"text = "{{ message }}""#,
            &vars(&[("message", "a\\b\nc\td")]),
        )
        .expect("should render");
        assert_eq!(out, r#"text = "a\\b\nc\td""#);
        toml::from_str::<toml::Value>(&out).expect("must be valid TOML");
    }

    #[test]
    fn a_numeric_field_passes_through_untouched() {
        // Escaping must not quote or alter a value used in a non-string
        // field, or `x = {{ pos }}` stops working.
        let out = render("t", "x = {{ pos }}", &vars(&[("pos", "4")])).expect("should render");
        assert_eq!(out, "x = 4");
    }

    #[test]
    fn the_safe_filter_opts_out_of_escaping() {
        // Verified: without an `is_safe()` check in the formatter this silently
        // escapes anyway, which would make `| safe` a lie.
        let out = render(
            "t",
            r#"raw = {{ value | safe }}"#,
            &vars(&[("value", "[1, 2]")]),
        )
        .expect("should render");
        assert_eq!(out, "raw = [1, 2]");
    }

    #[test]
    fn a_missing_variable_is_an_error_not_an_empty_string() {
        // Strict mode. Rendering `text = ""` would hit the device's
        // minLength: 1 as a confusing 400 instead of a local error.
        let error = render("greet", r#"text = "{{ message }}""#, &vars(&[]))
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("greet"), "should name the template, got {error}");
    }

    #[test]
    fn analyse_reports_the_variables_a_template_references() {
        let found = analyse("t", r#"text = "{{ message }}"
x = {{ pos }}"#)
            .expect("should analyse");
        assert_eq!(found, vec!["message".to_owned(), "pos".to_owned()]);
    }

    #[test]
    fn inheritance_constructs_are_rejected() {
        // `undeclared_variables` does not follow these, so a template using
        // one would silently under-report what it needs.
        for bad in [
            "{% include 'other.toml' %}",
            "{% import 'x.toml' as x %}",
            "{% from 'x.toml' import y %}",
            "{% extends 'base.toml' %}",
        ] {
            let error = analyse("t", bad)
                .expect_err(&format!("{bad} should be rejected"))
                .to_string();
            assert!(
                error.contains("self-contained"),
                "should explain why, got {error}"
            );
        }
    }

    #[test]
    fn a_syntax_error_names_the_template() {
        let error = analyse("broken", "text = \"{{ unclosed \"")
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("broken"), "got {error}");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test render`
Expected: FAIL — `analyse` and `render` are not defined.

- [ ] **Step 4: Implement**

Prepend to `src/template/render.rs`:

```rust
//! Rendering a template's text.
//!
//! The only module that imports `minijinja`, for the same reason `device.rs`
//! is the only one importing `busylib`: one file to fix when an upstream
//! layout moves.
//!
//! Substitution happens over the raw TOML text, before it is parsed, which is
//! what lets a variable appear in any field. That also means an unescaped
//! quote in a value would corrupt the document — and `message` routinely
//! arrives from `git log -1 --format=%s`. So every substitution is escaped for
//! a TOML basic string unless the template explicitly asks otherwise.

use std::collections::BTreeMap;

use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};

use crate::error::CliError;

/// Constructs whose targets `undeclared_variables` does not follow. A template
/// using one would under-report its required variables, so they are refused.
const FORBIDDEN: [&str; 4] = ["include", "import", "from", "extends"];

/// Escape for a TOML basic string. Digits, letters, and ordinary punctuation
/// pass through, so a numeric field is unaffected.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_name| AutoEscape::Custom("toml"));
    env.set_formatter(|out, state, value: &Value| {
        // `is_safe()` is what makes `| safe` mean anything. Without this check
        // the filter is silently ignored — verified by running it.
        if state.auto_escape() == AutoEscape::Custom("toml") && !value.is_safe() {
            out.write_str(&escape(&value.to_string()))?;
            return Ok(());
        }
        minijinja::escape_formatter(out, state, value)
    });
    env
}

/// Refuse the constructs `undeclared_variables` cannot see through.
fn reject_forbidden(name: &str, source: &str) -> Result<(), CliError> {
    for keyword in FORBIDDEN {
        let needle = format!("{{% {keyword}");
        let compact = format!("{{%{keyword}");
        if source.contains(&needle) || source.contains(&compact) {
            return Err(CliError::usage(format!(
                "template `{name}` uses `{{% {keyword} %}}`, which is not supported: \
                 templates must be self-contained single files, because the analysis \
                 that reports a template's required variables cannot see through it."
            )));
        }
    }
    Ok(())
}

/// The variables `source` references, sorted. Static analysis, so it runs
/// before rendering and produces a real error rather than a render failure.
///
/// Over-reports rather than under-reports: a variable mentioned only inside a
/// never-taken branch is still listed. `template show` says so.
pub fn analyse(name: &str, source: &str) -> Result<Vec<String>, CliError> {
    reject_forbidden(name, source)?;

    let mut env = environment();
    env.add_template(name.to_owned().leak(), source.to_owned().leak())
        .map_err(|error| syntax_error(name, &error))?;
    let template = env
        .get_template(name)
        .map_err(|error| syntax_error(name, &error))?;

    let mut found: Vec<String> = template.undeclared_variables(false).into_iter().collect();
    found.sort();
    Ok(found)
}

/// Render `source` with `vars`, escaping every substitution.
pub fn render(
    name: &str,
    source: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, CliError> {
    reject_forbidden(name, source)?;

    let mut env = environment();
    env.add_template(name.to_owned().leak(), source.to_owned().leak())
        .map_err(|error| syntax_error(name, &error))?;
    let template = env
        .get_template(name)
        .map_err(|error| syntax_error(name, &error))?;

    template
        .render(vars)
        .map_err(|error| syntax_error(name, &error))
}

fn syntax_error(name: &str, error: &minijinja::Error) -> CliError {
    let mut message = format!("template `{name}`: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    CliError::usage(message)
}
```

**On `.leak()`:** `Environment` borrows its template sources, and these are read from a file at runtime. Leaking two short strings per invocation, in a process that renders one template and exits, is simpler than threading a lifetime through every caller. Note it and move on; do not build an arena.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test render`
Expected: PASS, 8 tests.

Then `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/template/
git commit -m "feat: template rendering with TOML-safe auto-escaping"
```

---

## Task 3: `TemplateFile`, loading, and variable binding

**Files:**
- Modify: `src/template/mod.rs`
- Modify: `src/device.rs` (re-export `Priority`, `Color`, `DisplayElement` are present; add `ImageSource`)

**Interfaces:**
- Consumes: `discover::{root, validate_name, list, suggest}`, `render::{analyse, render}`.
- Produces:
  - `template::TemplateFile { description, priority, led_notification_color, elements }` with `into_payload(self, app: &str) -> Result<DisplayElements, CliError>`
  - `template::Template { name, dir, source }` with `load(root: &Path, name: &str) -> Result<Template, CliError>`, `required_variables(&self) -> Result<Vec<String>, CliError>`, `render(&self, vars) -> Result<TemplateFile, CliError>`
  - `template::bind_variables(positional: Option<&str>, pairs: &[String]) -> Result<BTreeMap<String, String>, CliError>`

- [ ] **Step 1: Re-export `ImageSource` from `src/device.rs`**

Task 2 of Phase 3 removed this re-export as speculative — nothing named the type. Task 8 now needs it to find the asset paths a template references, so it comes back, with a caller this time:

```rust
pub use busylib::model::assets::{ImageElement, ImageSource};
```

Merge it into the existing `pub use busylib::model::assets::{…}` block rather than adding a second statement beside it.

- [ ] **Step 2: Write the failing tests**

Append to `src/template/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{bind_variables, TemplateFile};

    #[test]
    fn a_template_file_becomes_a_payload_with_the_app_name_supplied() {
        // `DisplayElements::application_name` is required and has no default,
        // so a template cannot deserialize into it directly — verified. The
        // owning app comes from --app/BUSY_APP/config, never from the file.
        let file: TemplateFile = toml::from_str(
            r#"
            description = "an example"
            priority = 42
            [[elements]]
            id = "message"
            type = "text"
            text = "hi"
            font = "small"
            "#,
        )
        .expect("should parse");
        assert_eq!(file.description.as_deref(), Some("an example"));

        let payload = file.into_payload("busy").expect("should build");
        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(json.contains("\"application_name\":\"busy\""), "got {json}");
        assert!(json.contains("\"priority\":42"), "got {json}");
    }

    #[test]
    fn element_kinds_this_project_does_not_model_survive_the_round_trip() {
        // The whole reason a template deserializes into busylib's own types:
        // rectangle, countdown, and animation come along free.
        let file: TemplateFile = toml::from_str(
            r#"
            [[elements]]
            id = "bar"
            type = "rectangle"
            width = 40
            height = 4
            "#,
        )
        .expect("should parse");
        let payload = file.into_payload("busy").expect("should build");
        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(json.contains("\"type\":\"rectangle\""), "got {json}");
        assert!(json.contains("\"width\":40"), "got {json}");
    }

    #[test]
    fn an_unknown_key_is_an_error_rather_than_being_discarded() {
        // DisplayElements has no deny_unknown_fields, so `descriptoin = ".."`
        // would parse and vanish. The wrapper adds it back.
        let error = toml::from_str::<TemplateFile>("descriptoin = \"typo\"\nelements = []")
            .expect_err("should reject");
        assert!(error.to_string().contains("descriptoin"), "got {error}");
    }

    #[test]
    fn the_positional_binds_to_message() {
        let bound = bind_variables(Some("Build failed"), &[]).expect("should bind");
        assert_eq!(bound.get("message").map(String::as_str), Some("Build failed"));
    }

    #[test]
    fn var_pairs_are_parsed_and_may_contain_equals_signs() {
        let bound = bind_variables(None, &["code=500".to_owned(), "url=a=b".to_owned()])
            .expect("should bind");
        assert_eq!(bound.get("code").map(String::as_str), Some("500"));
        assert_eq!(bound.get("url").map(String::as_str), Some("a=b"));
    }

    #[test]
    fn a_var_without_an_equals_sign_is_a_usage_error() {
        let error = bind_variables(None, &["justakey".to_owned()])
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("justakey"), "got {error}");
        assert!(error.contains("k=v"), "should show the form, got {error}");
    }

    #[test]
    fn supplying_the_message_twice_is_an_error() {
        let error = bind_variables(Some("one"), &["message=two".to_owned()])
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("message"), "got {error}");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test template::tests`
Expected: FAIL — `TemplateFile` and `bind_variables` are not defined.

- [ ] **Step 4: Implement**

Replace the contents of `src/template/mod.rs` (keeping the test module at the bottom):

```rust
//! Templates: a directory holding a `template.toml` that renders to a payload.

pub mod discover;
pub mod render;
pub mod validate;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::device::{Color, DisplayElement, DisplayElements, Priority};
use crate::error::CliError;

/// A parsed `template.toml`.
///
/// This is `DisplayElements` minus `application_name`, plus `description`.
/// The architecture doc says a template deserializes directly into
/// `DisplayElements`; measured, it cannot — `application_name` is required and
/// has no default, and a template must not carry it, because which app owns
/// the draw comes from `--app`/`BUSY_APP`/the config file. `description` is
/// the other half: `DisplayElements` has no such field and no
/// `deny_unknown_fields`, so the doc's own example key parses and vanishes.
///
/// `elements` is `Vec<DisplayElement>` — busylib's own type — so `animation`,
/// `rectangle`, and `countdown` still come along free.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFile {
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub led_notification_color: Option<Color>,
    #[serde(default)]
    pub elements: Vec<DisplayElement>,
}

impl TemplateFile {
    /// Supply the envelope the template deliberately omits.
    pub fn into_payload(self, app: &str) -> Result<DisplayElements, CliError> {
        let app = crate::device::AppName::new(app.to_owned())
            .map_err(|error| CliError::usage(format!("invalid --app: {error}")))?;

        let mut payload = DisplayElements::new(app)
            .map_err(|error| CliError::usage(error.to_string()))?;

        if let Some(priority) = self.priority {
            payload = payload.priority(priority);
        }
        if let Some(color) = self.led_notification_color {
            payload = payload.led_notification_color(color);
        }
        for element in self.elements {
            payload = payload.element(element);
        }
        Ok(payload)
    }
}

/// A template on disk, loaded but not yet rendered.
#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub dir: PathBuf,
    pub source: String,
}

impl Template {
    /// Read `<root>/<name>/template.toml`.
    pub fn load(root: &Path, name: &str) -> Result<Self, CliError> {
        discover::validate_name(name)?;
        let dir = root.join(name);
        let path = dir.join("template.toml");

        let source = std::fs::read_to_string(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                let candidates = discover::list(root);
                let hint = match discover::suggest(name, &candidates) {
                    Some(near) => format!(" Did you mean `{near}`?"),
                    None if candidates.is_empty() => {
                        " No templates are installed; run `busy template init`.".to_owned()
                    }
                    None => format!(" Available: {}.", candidates.join(", ")),
                };
                CliError::usage(format!("no template named `{name}`.{hint}"))
            } else {
                CliError::usage(format!("could not read {}: {error}", path.display()))
            }
        })?;

        Ok(Self {
            name: name.to_owned(),
            dir,
            source,
        })
    }

    /// The variables this template references.
    pub fn required_variables(&self) -> Result<Vec<String>, CliError> {
        render::analyse(&self.name, &self.source)
    }

    /// Render and parse. The two are one step because a rendered template that
    /// does not parse is a template error, reported against the template name.
    pub fn render(&self, vars: &BTreeMap<String, String>) -> Result<TemplateFile, CliError> {
        let rendered = render::render(&self.name, &self.source, vars)?;
        toml::from_str::<TemplateFile>(&rendered).map_err(|error| {
            CliError::usage(format!(
                "template `{}` did not produce a valid template file: {error}",
                self.name
            ))
        })
    }
}

/// Collect variable values from the positional argument and repeated `--var`.
///
/// The positional binds to `message`, which is the one variable common enough
/// to deserve a positional. Supplying it both ways is an error rather than a
/// silent precedence rule.
pub fn bind_variables(
    positional: Option<&str>,
    pairs: &[String],
) -> Result<BTreeMap<String, String>, CliError> {
    let mut vars = BTreeMap::new();

    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(CliError::usage(format!(
                "`--var {pair}` is not in `k=v` form; write `--var {pair}=<value>`."
            )));
        };
        if key.is_empty() {
            return Err(CliError::usage(format!(
                "`--var {pair}` has an empty variable name."
            )));
        }
        vars.insert(key.to_owned(), value.to_owned());
    }

    if let Some(message) = positional {
        if vars.contains_key("message") {
            return Err(CliError::usage(
                "the message was supplied both as a positional argument and as \
                 `--var message=…`; use one or the other.",
            ));
        }
        vars.insert("message".to_owned(), message.to_owned());
    }

    Ok(vars)
}
```

Create `src/template/validate.rs` with just `//! Offline template checks.` for now; Task 4 fills it in. Add `pub mod validate;` is already in the block above.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test template`
Expected: PASS — 7 new tests plus Tasks 1-2's.

Then `cargo test`, clippy, fmt.

- [ ] **Step 6: Commit**

```bash
git add src/template/ src/device.rs
git commit -m "feat: TemplateFile, template loading, and variable binding"
```

---

## Task 4: Offline validation

**Files:**
- Modify: `src/template/validate.rs`

**Interfaces:**
- Consumes: `TemplateFile`, `device::{DisplayElements, ElementKind, ImageSource}`, `sanitize::to_ascii`, `crate::validate::bounds_warnings`.
- Produces:
  - `template::validate::Report { errors: Vec<String>, warnings: Vec<String> }`
  - `template::validate::offline(payload: &DisplayElements, dir: &Path) -> Report`
  - `template::validate::referenced_assets(payload: &DisplayElements) -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

Append to `src/template/validate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{offline, referenced_assets};
    use crate::template::TemplateFile;
    use std::path::Path;

    fn payload(toml_src: &str) -> crate::device::DisplayElements {
        toml::from_str::<TemplateFile>(toml_src)
            .expect("test fixture should parse")
            .into_payload("busy")
            .expect("test fixture should build")
    }

    #[test]
    fn duplicate_element_ids_are_an_error() {
        // Ids are the handle for --keep updates, so duplicates make a
        // template's own elements overwrite each other.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "a"
                type = "text"
                text = "one"
                font = "small"
                [[elements]]
                id = "a"
                type = "text"
                text = "two"
                font = "small"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert_eq!(report.errors.len(), 1, "got {:?}", report.errors);
        assert!(report.errors[0].contains("a"), "should name the id");
    }

    #[test]
    fn a_referenced_local_file_that_is_missing_is_an_error() {
        let dir = std::env::temp_dir().join(format!("busy-tv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "icon"
                type = "image"
                path = "stop.png"
                "#,
            ),
            &dir,
        );
        assert_eq!(report.errors.len(), 1, "got {:?}", report.errors);
        assert!(report.errors[0].contains("stop.png"), "should name the file");
    }

    #[test]
    fn a_stock_path_is_never_treated_as_a_local_file() {
        // `shared/…` is a device built-in; there is no local file to find.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "icon"
                type = "image"
                stock_path = "shared/checkmark_front_8x8.image"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert!(report.errors.is_empty(), "got {:?}", report.errors);
        assert!(referenced_assets(&payload(
            r#"
            [[elements]]
            id = "icon"
            type = "image"
            stock_path = "shared/checkmark_front_8x8.image"
            "#
        ))
        .is_empty());
    }

    #[test]
    fn non_ascii_text_warns_rather_than_failing() {
        // `busy text` transliterates and warns; a template is not more
        // fragile than a message.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "m"
                type = "text"
                text = "don't"
                font = "small"
                "#
                .replace('\'', "\u{2019}")
                .as_str(),
            ),
            Path::new("/nonexistent"),
        );
        assert!(report.errors.is_empty(), "got {:?}", report.errors);
        assert_eq!(report.warnings.len(), 1, "got {:?}", report.warnings);
        assert!(report.warnings[0].contains("ASCII"), "got {:?}", report.warnings);
    }

    #[test]
    fn bounds_warnings_come_free_from_the_existing_validator() {
        // Once rendered, a template IS a DisplayElements, so the payload
        // validator applies unchanged. This is the payoff for deserializing
        // into busylib's own type.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "m"
                type = "text"
                text = "hi"
                font = "small"
                x = 900
                y = 0
                align = "top_left"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert!(
            report.warnings.iter().any(|w| w.contains("outside")),
            "got {:?}",
            report.warnings
        );
    }

    #[test]
    fn referenced_assets_lists_only_local_image_paths() {
        let found = referenced_assets(&payload(
            r#"
            [[elements]]
            id = "a"
            type = "image"
            path = "stop.png"
            [[elements]]
            id = "b"
            type = "text"
            text = "hi"
            font = "small"
            "#,
        ));
        assert_eq!(found, vec!["stop.png".to_owned()]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test template::validate`
Expected: FAIL — `offline` and `referenced_assets` are not defined.

- [ ] **Step 3: Implement**

Prepend to `src/template/validate.rs`:

```rust
//! Offline template checks.
//!
//! Nothing here touches the device. Bounds and overflow checking is not
//! reimplemented: once rendered, a template *is* a `DisplayElements`, so
//! `crate::validate::bounds_warnings` applies unchanged. That reuse is the
//! payoff for deserializing into busylib's own types.

use std::path::Path;

use crate::device::{DisplayElements, ElementKind, ImageSource};
use crate::sanitize;

/// What validation found. Errors block a draw; warnings do not.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The app-asset paths a payload references. Stock paths are excluded: they
/// are device built-ins with no local file and nothing to upload.
pub fn referenced_assets(payload: &DisplayElements) -> Vec<String> {
    payload
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementKind::Image(image) => match &image.source {
                ImageSource::Asset { path } => Some(path.to_string()),
                ImageSource::Stock { .. } => None,
            },
            _ => None,
        })
        .collect()
}

/// Check a rendered template against everything knowable without the device.
pub fn offline(payload: &DisplayElements, dir: &Path) -> Report {
    let mut report = Report::default();

    let mut seen: Vec<String> = Vec::new();
    for element in &payload.elements {
        let id = element.id.to_string();
        if seen.contains(&id) {
            report.errors.push(format!(
                "duplicate element id `{id}`: ids are the handle for `--keep` updates, so \
                 two elements sharing one overwrite each other."
            ));
        } else {
            seen.push(id);
        }

        if let ElementKind::Text(text) = &element.kind {
            let sanitized = sanitize::to_ascii(text.text.as_str());
            if sanitized.changed {
                report.warnings.push(format!(
                    "element `{}` contains characters the bar's bitmap-ASCII fonts cannot \
                     render; they will be transliterated or dropped.",
                    element.id
                ));
            }
        }
    }

    for asset in referenced_assets(payload) {
        if !dir.join(&asset).is_file() {
            report.errors.push(format!(
                "references `{asset}`, which is not in {}.",
                dir.display()
            ));
        }
    }

    report.warnings.extend(crate::validate::bounds_warnings(payload));
    report
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test template::validate`
Expected: PASS, 6 tests. Then `cargo test`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add src/template/validate.rs
git commit -m "feat: offline template validation reusing the payload validator"
```

---

## Task 5: `busy template list | show | validate`

**Files:**
- Create: `src/cmd/template.rs`, `tests/template.rs`
- Modify: `src/cli.rs`, `src/cmd/mod.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `Template`, `discover`, `template::validate::offline`.
- Produces: `cli::TemplateCmd`, `cmd::template::{list, show, validate}`.

- [ ] **Step 1: Write the failing tests**

Create `tests/template.rs`:

```rust
mod common;

use common::busy;

/// A template root with `error` (a required variable) and `plain` (none).
fn root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("busy-tpl-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(
        dir.join("error/template.toml"),
        r#"description = "Red error text"
[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
font = "small"
color = "#ff0000ff"
"#,
    )
    .expect("write");
    std::fs::create_dir_all(dir.join("plain")).expect("temp dir");
    std::fs::write(
        dir.join("plain/template.toml"),
        r#"description = "No variables"
[[elements]]
id = "message"
type = "text"
text = "static"
font = "small"
"#,
    )
    .expect("write");
    dir
}

#[test]
fn list_names_every_template_with_its_description() {
    let dir = root("list");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "list"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "got {stdout}");
    assert!(stdout.contains("Red error text"), "got {stdout}");
    assert!(stdout.contains("plain"), "got {stdout}");
}

#[test]
fn show_reports_the_required_variables() {
    let dir = root("show");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "show", "error"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("message"), "should name the variable, got {stdout}");
}

#[test]
fn show_of_a_misspelled_name_suggests_the_near_match() {
    let dir = root("suggest");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "show", "eror"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Did you mean `error`"), "got {stderr}");
}

#[test]
fn validate_accepts_a_good_template() {
    let dir = root("valid");
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate"])
        .assert()
        .success();
}

#[test]
fn validate_rejects_a_duplicate_element_id() {
    let dir = std::env::temp_dir().join("busy-tpl-dup");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("dup")).expect("temp dir");
    std::fs::write(
        dir.join("dup/template.toml"),
        r#"[[elements]]
id = "a"
type = "text"
text = "one"
font = "small"
[[elements]]
id = "a"
type = "text"
text = "two"
font = "small"
"#,
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate", "dup"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("duplicate element id"),
        "got {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test template`
Expected: FAIL — there is no `template` subcommand, so clap exits 2 with "unrecognized subcommand".

- [ ] **Step 3: Add the CLI surface**

In `src/cli.rs`, extend `Command`:

```rust
    /// Manage and run templates
    #[command(subcommand)]
    Template(TemplateCmd),
```

and add:

```rust
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
    Run(Box<DrawArgs>),
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
```

`Run` flattens `DrawArgs` rather than declaring a parallel set of placement and delivery options — two structs that must stay in step would drift on the first flag added to either. Task 7 wires it up; `--as` is simply ignored there, since `run`'s name is always a template.

Add `--var` to `DrawArgs`, after `opacity`:

```rust
    /// Template variable, repeatable: --var key=value
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Optional message; binds to the `message` template variable
    pub message: Option<String>,
```

The `message` positional must be declared **after** `name` so clap assigns them in order.

- [ ] **Step 4: Write `src/cmd/template.rs`**

```rust
//! `busy template` — manage and run templates.

use crate::cli::{TemplateShowArgs, TemplateValidateArgs};
use crate::config::Settings;
use crate::error::CliError;
use crate::output::Emitter;
use crate::template::{discover, validate, Template};

/// The template root, or a usage error explaining that there is no config
/// directory to put one in.
pub fn root(flag: Option<&std::path::Path>) -> Result<std::path::PathBuf, CliError> {
    discover::root(flag).ok_or_else(|| {
        CliError::usage(
            "could not determine a config directory for templates; pass --template-dir.",
        )
    })
}

pub fn list(root: &std::path::Path, emitter: &Emitter) -> Result<(), CliError> {
    let names = discover::list(root);
    if names.is_empty() {
        return emitter.success(
            &format!(
                "no templates in {}; run `busy template init` to write the examples",
                root.display()
            ),
            None,
        );
    }

    let mut report = String::new();
    for name in &names {
        let description = Template::load(root, name)
            .ok()
            .and_then(|template| {
                toml::from_str::<crate::template::TemplateFile>(&template.source)
                    .ok()
                    .and_then(|file| file.description)
            })
            .unwrap_or_default();
        report.push_str(&format!("{name}\t{description}\n"));
    }
    report.push_str(&format!("{} template(s)", names.len()));
    emitter.success(&report, None)
}

pub fn show(
    args: &TemplateShowArgs,
    root: &std::path::Path,
    emitter: &Emitter,
) -> Result<(), CliError> {
    let template = Template::load(root, &args.name)?;
    let file: crate::template::TemplateFile = toml::from_str(&template.source)
        .map_err(|error| CliError::usage(format!("template `{}`: {error}", args.name)))?;
    let variables = template.required_variables()?;

    let mut report = format!("{}\n", args.name);
    if let Some(description) = &file.description {
        report.push_str(&format!("  {description}\n"));
    }
    report.push_str(&format!("  {} element(s)\n", file.elements.len()));
    if variables.is_empty() {
        report.push_str("  no variables\n");
    } else {
        // Static analysis over-reports: a variable mentioned only inside a
        // never-taken branch is still listed. Say so rather than implying
        // every one is mandatory.
        report.push_str(&format!(
            "  variables mentioned: {}\n",
            variables.join(", ")
        ));
    }
    report.push_str(&format!("  {}", template.dir.display()));
    emitter.success(&report, None)
}

pub fn validate(
    args: &TemplateValidateArgs,
    root: &std::path::Path,
    settings: &Settings,
    emitter: &Emitter,
) -> Result<(), CliError> {
    let names = match &args.name {
        Some(name) => vec![name.clone()],
        None => discover::list(root),
    };

    if names.is_empty() {
        return emitter.success("no templates to validate", None);
    }

    let mut failures = Vec::new();
    for name in &names {
        let template = Template::load(root, name)?;

        // Bind every referenced variable to a placeholder so the render can
        // proceed: validation is about the template's shape, not about whether
        // the caller happened to supply values.
        let variables = template.required_variables()?;
        let vars = variables
            .into_iter()
            .map(|key| (key, "x".to_owned()))
            .collect();

        let file = template.render(&vars)?;
        let payload = file.into_payload(&settings.app)?;
        let report = validate::offline(&payload, &template.dir);

        for warning in &report.warnings {
            emitter.warn(&format!("{name}: {warning}"));
        }
        for error in &report.errors {
            failures.push(format!("{name}: {error}"));
        }
    }

    if !failures.is_empty() {
        return Err(CliError::usage(failures.join("\n")));
    }
    emitter.success(&format!("{} template(s) OK", names.len()), None)
}
```

Add `pub mod template;` to `src/cmd/mod.rs`.

- [ ] **Step 5: Wire it into `src/main.rs`**

Add to the `match`:

```rust
        Command::Template(command) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            let root = cmd::template::root(cli.global.template_dir.as_deref())?;
            match command {
                cli::TemplateCmd::List => cmd::template::list(&root, emitter),
                cli::TemplateCmd::Show(args) => cmd::template::show(args, &root, emitter),
                cli::TemplateCmd::Validate(args) => {
                    cmd::template::validate(args, &root, &settings, emitter)
                }
                cli::TemplateCmd::Init(_) => {
                    Err(CliError::runtime("`busy template init` arrives in Task 6"))
                }
                cli::TemplateCmd::Run(_) => {
                    Err(CliError::runtime("`busy template run` arrives in Task 7"))
                }
            }
        }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test template`
Expected: PASS, 5 tests. Then `cargo test`, clippy, fmt.

- [ ] **Step 7: Verify by running the binary**

```bash
mkdir -p /tmp/tpl/error && cat > /tmp/tpl/error/template.toml <<'EOF'
description = "Red error text"
[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
font = "small"
color = "#ff0000ff"
EOF
cargo run -- --template-dir /tmp/tpl template list
cargo run -- --template-dir /tmp/tpl template show error
cargo run -- --template-dir /tmp/tpl template show eror   # expect exit 2 + did-you-mean
cargo run -- --template-dir /tmp/tpl template validate
```

Read what a user actually sees. Paste it into the report.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs src/cmd/ src/main.rs tests/template.rs
git commit -m "feat: busy template list, show, and validate"
```

---

## Task 6: Shipped examples and `busy template init`

**Files:**
- Create: `templates/error/template.toml`, `templates/ok/template.toml`, `tests/examples.rs`
- Modify: `src/cmd/template.rs`, `src/main.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `Template`, `template::validate::offline`.
- Produces: `cmd::template::{EXAMPLES, init}`.

- [ ] **Step 1: Add the dependency**

```bash
cargo add include_dir
```

Expect `include_dir = "0.7.4"`. No features needed.

- [ ] **Step 2: Write the example templates**

`templates/error/template.toml`:

```toml
# A red error message beside a stop icon.
#
# `{{ message }}` has no default, so this template REQUIRES a message:
#   busy draw error "Build failed"
#
# The icon is a device built-in (`shared/…`), so nothing needs uploading.
description = "Red error text beside a stop icon"
priority = 95

[[elements]]
id = "icon"
type = "image"
stock_path = "shared/cross_front_8x8.image"
x = 4
y = 8
align = "mid_left"

[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
x = 16
y = 8
align = "mid_left"
font = "small"
color = "#ff0000ff"
```

`templates/ok/template.toml`:

```toml
# A green confirmation.
#
# `message` is OPTIONAL here — minijinja's own `default` filter supplies one,
# so `busy draw ok` works with no arguments:
#   busy draw ok
#   busy draw ok "Deploy finished"
description = "Green confirmation text"
priority = 95

[[elements]]
id = "message"
type = "text"
text = "{{ message | default('Done') }}"
x = 36
y = 8
align = "center"
font = "small"
color = "#00ff00ff"
```

**Verify the stock path exists before committing** — `scripts/probe-device.sh` step 8 lists the device's stock assets. If `cross_front_8x8.image` is not among them, pick one that is and update the comment. A shipped example that cannot draw is worse than no example.

- [ ] **Step 3: Write the failing tests**

Create `tests/examples.rs`:

```rust
//! Every shipped example must render, parse, and validate.
//!
//! This is what makes "adding an example is just a commit" safe. It iterates
//! the embedded set rather than naming `error` and `ok`, so it keeps covering
//! whatever is added next.

mod common;

use common::busy;

#[test]
fn init_writes_the_examples_and_they_all_validate() {
    let dir = std::env::temp_dir().join("busy-examples-init");
    let _ = std::fs::remove_dir_all(&dir);

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(dir.join("error/template.toml").is_file(), "error not written");
    assert!(dir.join("ok/template.toml").is_file(), "ok not written");

    // The guard: whatever init just wrote must pass full offline validation.
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate"])
        .assert()
        .success();
}

#[test]
fn init_skips_an_existing_template_without_force() {
    let dir = std::env::temp_dir().join("busy-examples-skip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(dir.join("error/template.toml"), "# mine\nelements = []\n").expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .output()
        .expect("should run");
    assert!(output.status.success());

    let kept = std::fs::read_to_string(dir.join("error/template.toml")).expect("read");
    assert!(kept.contains("# mine"), "an existing template must not be clobbered");
    assert!(dir.join("ok/template.toml").is_file(), "new examples still arrive");
}

#[test]
fn init_force_overwrites() {
    let dir = std::env::temp_dir().join("busy-examples-force");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(dir.join("error/template.toml"), "# mine\nelements = []\n").expect("write");

    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init", "--force"])
        .assert()
        .success();

    let replaced = std::fs::read_to_string(dir.join("error/template.toml")).expect("read");
    assert!(!replaced.contains("# mine"), "--force must overwrite");
}

#[test]
fn every_example_declares_a_description() {
    // `template list` prints it; an example without one is a documentation
    // gap, and these examples ARE the documentation.
    let dir = std::env::temp_dir().join("busy-examples-desc");
    let _ = std::fs::remove_dir_all(&dir);
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .assert()
        .success();

    for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
        let toml_path = entry.path().join("template.toml");
        if !toml_path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&toml_path).expect("read");
        assert!(
            source.contains("description ="),
            "{} has no description",
            entry.path().display()
        );
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --test examples`
Expected: FAIL — `template init` returns the Task 6 placeholder error.

- [ ] **Step 5: Implement `init`**

Append to `src/cmd/template.rs`:

```rust
use include_dir::{include_dir, Dir};

/// The shipped example templates, embedded at compile time.
///
/// Adding an example is a commit to `templates/`, not a code change: `init`
/// walks this tree rather than a hand-maintained list. Whole directories are
/// embedded rather than single files, so a future example carrying a PNG works
/// with no change here.
static EXAMPLES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

pub fn init(
    args: &crate::cli::TemplateInitArgs,
    root: &std::path::Path,
    emitter: &Emitter,
) -> Result<(), CliError> {
    std::fs::create_dir_all(root).map_err(|error| {
        CliError::runtime(format!("could not create {}: {error}", root.display()))
    })?;

    let mut written = Vec::new();
    let mut skipped = Vec::new();

    for dir in EXAMPLES.dirs() {
        let Some(name) = dir.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `get_file` wants the path as stored, which includes the directory
        // prefix — `get_file("template.toml")` returns None.
        if dir.get_file(dir.path().join("template.toml")).is_none() {
            continue;
        }

        let destination = root.join(name);
        if destination.exists() && !args.force {
            skipped.push(name.to_owned());
            continue;
        }

        std::fs::create_dir_all(&destination).map_err(|error| {
            CliError::runtime(format!("could not create {}: {error}", destination.display()))
        })?;

        for file in dir.files() {
            let Some(leaf) = file.path().file_name() else {
                continue;
            };
            let target = destination.join(leaf);
            std::fs::write(&target, file.contents()).map_err(|error| {
                CliError::runtime(format!("could not write {}: {error}", target.display()))
            })?;
        }
        written.push(name.to_owned());
    }

    let mut report = String::new();
    if !written.is_empty() {
        report.push_str(&format!("wrote {} to {}\n", written.join(", "), root.display()));
    }
    if !skipped.is_empty() {
        report.push_str(&format!(
            "kept your existing {} (use --force to replace)\n",
            skipped.join(", ")
        ));
    }
    if report.is_empty() {
        report.push_str("no examples are bundled with this build");
    }
    emitter.success(report.trim_end(), None)
}
```

Replace the `Init` arm in `src/main.rs`:

```rust
                cli::TemplateCmd::Init(args) => cmd::template::init(args, &root, emitter),
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test examples`
Expected: PASS, 4 tests. Then `cargo test`, clippy, fmt.

- [ ] **Step 7: Verify by running the binary**

```bash
rm -rf /tmp/tpl2
cargo run -- --template-dir /tmp/tpl2 template init
cargo run -- --template-dir /tmp/tpl2 template init      # expect "kept your existing ..."
cargo run -- --template-dir /tmp/tpl2 template list
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock templates/ src/cmd/template.rs src/main.rs tests/examples.rs
git commit -m "feat: shipped example templates and busy template init"
```

---

## Task 7: The override table, and `draw`/`run` for templates

**Files:**
- Create: `src/overrides.rs`, `tests/overrides.rs`
- Modify: `src/cmd/draw.rs`, `src/main.rs`, `src/cli.rs`, `tests/draw.rs`

**Interfaces:**
- Consumes: `TemplateFile`, `Template`, `bind_variables`, `discover`.
- Produces:
  - `overrides::Kind { Image, Stock, Template, File }` — four variants; `text` never
    reaches this path, because `busy text` builds its own payload and has no `--file`
    or template form.
  - `overrides::apply(payload: DisplayElements, args: &DrawArgs, kind: Kind) -> Result<DisplayElements, CliError>`
  - `overrides::reject_vars_unless_template(args: &DrawArgs, kind: Kind) -> Result<(), CliError>`
  - `cmd::draw::Resolved::Template(Box<Template>)`

- [ ] **Step 1: Write the failing tests**

Create `tests/overrides.rs`:

```rust
mod common;

use common::busy;

fn root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("busy-ovr-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("two")).expect("temp dir");
    std::fs::write(
        dir.join("two/template.toml"),
        r#"priority = 40
[[elements]]
id = "a"
type = "text"
text = "one"
font = "small"
[[elements]]
id = "b"
type = "text"
text = "two"
font = "small"
"#,
    )
    .expect("write");
    dir
}

#[test]
fn payload_level_flags_override_a_template() {
    let dir = root("payload");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "two", "--priority", "high", "--led", "blue"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"priority\": 95"), "got {stdout}");
    assert!(stdout.contains("led_notification_color"), "got {stdout}");
}

#[test]
fn an_absent_flag_leaves_the_templates_own_value_alone() {
    // Substituting a default here would silently overwrite a value the
    // template never asked to have replaced.
    let dir = root("absent");
    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "two"])
        .output()
        .expect("should run");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"priority\": 40"),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn per_element_flags_are_rejected_on_a_template() {
    let dir = root("perelement");
    for flag in [
        vec!["-x", "4"],
        vec!["-y", "2"],
        vec!["--align", "center"],
        vec!["--screen", "back"],
        vec!["--timeout", "30"],
        vec!["--opacity", "50"],
        vec!["--id", "mine"],
    ] {
        let output = busy()
            .args(["--template-dir"])
            .arg(&dir)
            .args(["--dry-run", "draw", "two"])
            .args(&flag)
            .output()
            .expect("should run");
        assert_eq!(output.status.code(), Some(2), "{flag:?} should be rejected");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(flag[0]), "error must name {}, got {stderr}", flag[0]);
    }
}

#[test]
fn var_is_rejected_on_a_non_template_draw() {
    // A flag that parses but does nothing is a defect.
    let output = busy()
        .args(["--dry-run", "draw", "logo.png", "--var", "a=1"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--var"));
}

#[test]
fn a_second_positional_on_an_image_draw_names_the_near_match() {
    let dir = root("typo");
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(
        dir.join("error/template.toml"),
        "[[elements]]\nid = \"m\"\ntype = \"text\"\ntext = \"{{ message }}\"\nfont = \"small\"\n",
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "eror", "Build failed"])
        .output()
        .expect("should run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Did you mean `error`"), "got {stderr}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test overrides`
Expected: FAIL — templates do not resolve in `draw` yet.

- [ ] **Step 3: Write `src/overrides.rs`**

```rust
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
        ("-x/--x", args.placement.x.is_some()),
        ("-y/--y", args.placement.y.is_some()),
        ("--align", args.placement.align.is_some()),
        ("--screen", args.placement.screen.is_some()),
        ("--timeout", args.delivery.timeout.is_some()),
        ("--opacity", args.opacity.is_some()),
        ("--id", args.delivery.id.is_some()),
    ];

    for (flag, given) in per_element {
        if given {
            return Err(rejected(flag, kind));
        }
    }

    if let Some(input) = &args.delivery.priority {
        let value = crate::config::parse_priority(input)?;
        let priority = Priority::new(value)
            .map_err(|error| CliError::usage(format!("invalid --priority: {error}")))?;
        payload.priority = Some(priority);
    }

    if let Some(input) = &args.delivery.led {
        payload.led_notification_color = Some(crate::color::parse(input)?);
    }

    Ok(payload)
}

/// `--var` only means something for a template.
pub fn reject_vars_unless_template(args: &DrawArgs, kind: Kind) -> Result<(), CliError> {
    if !args.vars.is_empty() && kind != Kind::Template {
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
    CliError::usage(format!("{flag} cannot be used with {}: {advice}", kind.describe()))
}
```

Add `mod overrides;` to `src/main.rs`, alphabetically between `output` and `sanitize`.

- [ ] **Step 4: Add template resolution to `src/cmd/draw.rs`**

Extend `Resolved`:

```rust
pub enum Resolved {
    Asset(AssetPath),
    Stock(StockPath),
    Template(Box<crate::template::Template>),
}
```

Replace `resolve` with a version taking the template root — this is resolution rule 2, the insertion Phase 3 shaped this function to accept:

```rust
pub fn resolve(args: &DrawArgs, root: &std::path::Path) -> Result<Resolved, CliError> {
    let name = args.name.as_deref().ok_or_else(|| {
        CliError::usage("`busy draw` needs a name or --file; see `busy draw --help`")
    })?;

    // 1. `shared/…` is the spec's reserved namespace for device built-ins.
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

    // 2. A template directory of this name.
    if forced_template || (!forced_image && root.join(name).join("template.toml").is_file()) {
        let template = crate::template::Template::load(root, name)?;
        return Ok(Resolved::Template(Box::new(template)));
    }

    // 3. Otherwise an app asset. A message here cannot be meant for an image,
    //    so it is the typo guard: report the near-match instead of sending a
    //    doomed asset draw.
    if args.message.is_some() {
        let candidates = crate::template::discover::list(root);
        let hint = match crate::template::discover::suggest(name, &candidates) {
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
```

Add `Template` to `cli::AsArg`:

```rust
pub enum AsArg {
    Image,
    Stock,
    Template,
}
```

Delete `apply_file_overrides` and `file_override_rejected` from `cmd/draw.rs` — `overrides::apply` replaces both.

- [ ] **Step 5: Wire it into `src/main.rs`**

Replace the `Command::Draw` arm's payload construction:

```rust
        Command::Draw(args) => {
            let settings = config::resolve(&cli.global, &cli::StyleArgs::default(), &env, &file)?;
            let root = cmd::template::root(cli.global.template_dir.as_deref())?;

            if args.delivery.until.is_some() {
                return Err(CliError::usage(
                    "--until is not yet supported on `draw`; use --timeout instead",
                ));
            }

            let (payload, kind) = match &args.file {
                Some(path) => (cmd::draw::load_file(path)?, overrides::Kind::File),
                None => match cmd::draw::resolve(args, &root)? {
                    cmd::draw::Resolved::Template(template) => {
                        let vars = template::bind_variables(
                            args.message.as_deref(),
                            &args.vars,
                        )?;
                        let rendered = template.render(&vars)?;
                        let payload = rendered.into_payload(&settings.app)?;

                        let report = template::validate::offline(&payload, &template.dir);
                        if !report.is_ok() {
                            return Err(CliError::usage(report.errors.join("\n")));
                        }
                        for warning in &report.warnings {
                            emitter.warn(warning);
                        }
                        (payload, overrides::Kind::Template)
                    }
                    resolved => {
                        // Compute the kind before building, so `resolved` is
                        // still available to borrow.
                        let kind = if matches!(resolved, cmd::draw::Resolved::Stock(_)) {
                            overrides::Kind::Stock
                        } else {
                            overrides::Kind::Image
                        };
                        (
                            cmd::draw::build_payload(args, &settings, &file, &resolved)?,
                            kind,
                        )
                    }
                },
            };

            overrides::reject_vars_unless_template(args, kind)?;
            let payload = overrides::apply(payload, args, kind)?;

            for warning in validate::bounds_warnings(&payload) {
                emitter.warn(&warning);
            }

            if cli.global.dry_run {
                return emitter.dry_run(&payload);
            }

            let device = device::Device::connect(&settings)?;
            if !args.delivery.keep {
                device.clear().await?;
            }
            device.draw(&payload).await?;

            emitter.success("drawn", Some(&payload))
        }
```

Replace the `Run` arm in the `Template` match to delegate to the same code — the simplest correct form is to construct the equivalent `DrawArgs` and reuse it:

```rust
                cli::TemplateCmd::Run(args) => {
                    // `run` is `draw` with the name always read as a template.
                    let mut args = args.clone();
                    args.as_kind = Some(cli::AsArg::Template);
                    return Box::pin(run(
                        &Cli {
                            global: cli.global.clone(),
                            command: Command::Draw(args),
                        },
                        emitter,
                    ))
                    .await;
                }
```

`Cli` and `GlobalArgs` need `#[derive(Clone)]` for this; `GlobalArgs` already has it, so add `Clone` to `Cli` and `Command`. The `Box::pin` is required because `run` is recursive and `async fn` cannot be directly recursive.

Note the `--id` check for `--file` moves into `overrides::apply` and is deleted from this arm.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test overrides` then `cargo test`
Expected: PASS. The existing `tests/draw.rs` per-element rejection tests must still pass unchanged — they now exercise `overrides::apply` instead of `apply_file_overrides`, which is the point. If any `.snap` file changes, STOP and report: the wire payload moved.

Then clippy and fmt.

- [ ] **Step 7: Verify by running the binary**

```bash
cargo run -- --template-dir /tmp/tpl2 --dry-run draw ok
cargo run -- --template-dir /tmp/tpl2 --dry-run draw error "Build failed"
cargo run -- --template-dir /tmp/tpl2 --dry-run draw error          # expect exit 2, names `message`
cargo run -- --template-dir /tmp/tpl2 --dry-run draw error 'He said "hi"'   # escaping
cargo run -- --template-dir /tmp/tpl2 --dry-run template run ok
```

The quoted-message case is the one to read carefully: the payload must contain the quotes intact.

- [ ] **Step 8: Commit**

```bash
git add src/overrides.rs src/cmd/draw.rs src/cli.rs src/main.rs tests/overrides.rs
git commit -m "feat: one override table for templates and --file, and draw resolution rule 2"
```

---

## Task 8: The asset presence check

**Files:**
- Modify: `src/cmd/draw.rs` or `src/main.rs` (wherever the template branch lands), `tests/template.rs`

**Interfaces:**
- Consumes: `Device::list_assets`, `template::validate::referenced_assets`.
- Produces: `cmd::template::check_assets_present(device, payload, dir, name) -> Result<(), CliError>`

- [ ] **Step 1: Write the failing test**

Append to `tests/template.rs`:

```rust
use wiremock::matchers::{method, path as mock_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn a_template_referencing_an_absent_asset_names_the_upload_command() {
    let dir = std::env::temp_dir().join("busy-tpl-asset");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logo")).expect("temp dir");
    std::fs::write(
        dir.join("logo/template.toml"),
        "[[elements]]\nid = \"i\"\ntype = \"image\"\npath = \"stop.png\"\n",
    )
    .expect("write");
    // The local file exists, so offline validation passes; the device does not
    // have it, which is what this check is for.
    std::fs::write(dir.join("logo/stop.png"), b"not really a png").expect("write");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(mock_path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"list": []})))
        .mount(&server)
        .await;

    let output = common::busy_at(&server)
        .args(["--template-dir"])
        .arg(&dir)
        .args(["draw", "logo"])
        .output()
        .expect("should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stop.png"), "got {stderr}");
    assert!(stderr.contains("busy asset upload"), "must name the fix, got {stderr}");
}

#[tokio::test]
async fn a_failed_listing_does_not_block_the_draw() {
    // `/ext/user_assets/<app>/` is undocumented. A firmware change must not
    // break the tool; it may only make the resulting error later and worse.
    let dir = std::env::temp_dir().join("busy-tpl-degrade");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logo")).expect("temp dir");
    std::fs::write(
        dir.join("logo/template.toml"),
        "[[elements]]\nid = \"i\"\ntype = \"image\"\npath = \"stop.png\"\n",
    )
    .expect("write");
    std::fs::write(dir.join("logo/stop.png"), b"not really a png").expect("write");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(mock_path("/api/storage/list"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("DELETE")).respond_with(common::ok()).mount(&server).await;
    Mock::given(method("POST"))
        .and(mock_path("/api/display/draw"))
        .respond_with(common::ok())
        .expect(1)
        .mount(&server)
        .await;

    let output = common::busy_at(&server)
        .args(["--template-dir"])
        .arg(&dir)
        .args(["draw", "logo"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "a failed listing must not block: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test template asset`
Expected: FAIL — no presence check exists, so the draw proceeds and the first test's exit code is not 2.

- [ ] **Step 3: Implement**

Append to `src/cmd/template.rs`:

```rust
/// Confirm every app asset a template references is on the device.
///
/// 4a checks and explains; 4b uploads. This is step 1 of the sync described in
/// the command-surface spec §5.5, so 4b adds steps 2-4 rather than replacing it.
///
/// `Device::list_assets` already maps the directory-missing 400 to an empty
/// list, so `Ok(entries)` is authoritative — a name absent from an empty list
/// is genuinely not on the device. An `Err` means the listing itself failed,
/// and the check is skipped entirely: this must never be the reason a draw
/// fails.
pub async fn check_assets_present(
    device: &crate::device::Device,
    payload: &crate::device::DisplayElements,
    dir: &std::path::Path,
    name: &str,
) -> Result<(), CliError> {
    let referenced = validate::referenced_assets(payload);
    if referenced.is_empty() {
        return Ok(());
    }

    let Ok(entries) = device.list_assets().await else {
        return Ok(());
    };
    let present: Vec<String> = entries.iter().map(|e| e.name().to_owned()).collect();

    let missing: Vec<String> = referenced
        .into_iter()
        .filter(|asset| !present.contains(asset))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    let uploads: Vec<String> = missing
        .iter()
        .map(|asset| format!("  busy asset upload {}", dir.join(asset).display()))
        .collect();

    Err(CliError::usage(format!(
        "template `{name}` references {}, which {} not uploaded.\nRun:\n{}",
        missing.join(", "),
        if missing.len() == 1 { "is" } else { "are" },
        uploads.join("\n")
    )))
}
```

The check needs a device, so it runs after `Device::connect` — but it needs the template's
directory and name, which only exist inside the match. Carry them out in a third tuple slot.

In `src/main.rs`, change the Draw arm's binding from `let (payload, kind) = …` to
`let (payload, kind, template_context) = …`, and make each arm produce the third value:

```rust
            let (payload, kind, template_context): (_, _, Option<(std::path::PathBuf, String)>) =
                match &args.file {
                    Some(path) => (cmd::draw::load_file(path)?, overrides::Kind::File, None),
                    None => match cmd::draw::resolve(args, &root)? {
                        cmd::draw::Resolved::Template(template) => {
                            let vars =
                                template::bind_variables(args.message.as_deref(), &args.vars)?;
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
                                overrides::Kind::Template,
                                Some((template.dir.clone(), template.name.clone())),
                            )
                        }
                        resolved => {
                            let kind = if matches!(resolved, cmd::draw::Resolved::Stock(_)) {
                                overrides::Kind::Stock
                            } else {
                                overrides::Kind::Image
                            };
                            (
                                cmd::draw::build_payload(args, &settings, &file, &resolved)?,
                                kind,
                                None,
                            )
                        }
                    },
                };
```

Then insert the check immediately after `Device::connect`, before the `--keep`-gated clear:

```rust
            let device = device::Device::connect(&settings)?;

            if let Some((dir, name)) = &template_context {
                cmd::template::check_assets_present(&device, &payload, dir, name).await?;
            }

            if !args.delivery.keep {
                device.clear().await?;
            }
```

Placing it before the `clear()` matters: a template with a missing asset must not first wipe
what is already on the panel and then refuse to draw.

**The check must not run under `--dry-run`** — the dry-run return happens before `Device::connect`, so this is automatic. Confirm it by running `busy --dry-run draw logo` against a template with an asset and observing that nothing is contacted.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test template`
Expected: PASS. Then `cargo test`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add src/cmd/template.rs src/main.rs tests/template.rs
git commit -m "feat: check a template's assets are on the device before drawing"
```

---

## Task 9: Golden snapshot and spec corrections

**Files:**
- Modify: `tests/template.rs`, `docs/busy-cli-architecture.md`, `docs/specs/2026-08-09-busy-cli-ux-design.md`

**Interfaces:** none new.

- [ ] **Step 1: Add the golden snapshot**

Append to `tests/template.rs`:

```rust
#[test]
fn golden_payload_for_a_rendered_multi_element_template() {
    // Pins that a template really does produce the same wire bytes a
    // hand-written payload would, including that `rectangle` — an element
    // kind this project does not model — survives untouched.
    let dir = std::env::temp_dir().join("busy-tpl-golden");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("status")).expect("temp dir");
    std::fs::write(
        dir.join("status/template.toml"),
        r#"description = "text plus a progress bar"
priority = 95
[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
x = 2
y = 8
align = "mid_left"
font = "small"
[[elements]]
id = "bar"
type = "rectangle"
width = 40
height = 3
x = 2
y = 14
align = "mid_left"
"#,
    )
    .expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "status", "Deploying"])
        .output()
        .expect("should run");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    insta::assert_snapshot!(String::from_utf8_lossy(&output.stdout));
}
```

- [ ] **Step 2: Generate and inspect the snapshot**

`cargo insta review` is interactive and will hang. Use:

```bash
INSTA_UPDATE=always cargo test --test template golden
```

Then **read** `tests/snapshots/template__golden_payload_for_a_rendered_multi_element_template.snap` before accepting. It must contain `"application_name": "busy"`, `"priority": 95`, a `"type": "text"` element with `"text": "Deploying"`, and a `"type": "rectangle"` element with `"width": 40`. If it does not, investigate — do not accept it and do not edit the expectation to match.

- [ ] **Step 3: Correct the architecture doc**

In `docs/busy-cli-architecture.md` §7, the sentence "**The key move: a template deserializes directly into `busylib::model::assets::DisplayElements`.**" is measurably false. Replace that paragraph with:

```markdown
**The key move: a template's `elements` deserialize directly into
`busylib::model::assets::DisplayElement`.** The template file is the API payload
minus its envelope, which means animation, countdown, and rectangle elements
come along for free without this project modeling them.

A template does *not* deserialize into `DisplayElements` itself: that type
requires `application_name`, which comes from `--app`/`BUSY_APP`/the config
file and must not be baked into a template. A thin `TemplateFile` wrapper
supplies the envelope and adds the `description` field this document's own
example uses. See `docs/specs/2026-08-11-phase-4a-templates-design.md` §2.1.
```

Also correct the bullet "**Substitution:** `minijinja` over the raw TOML text *before* parsing" by appending: "Every substitution is auto-escaped for a TOML basic string, so a quote in a commit subject cannot corrupt the document; `| safe` opts out."

- [ ] **Step 4: Correct the command-surface spec**

In `docs/specs/2026-08-09-busy-cli-ux-design.md` §3.3, replace the paragraph beginning "`busy draw`'s `--help` is the union of Style, Placement, Scroll, Delivery, `--var` and `--opacity`" with:

```markdown
`busy draw` offers Placement, Delivery, `--opacity`, and `--var`. It deliberately
does **not** offer Style or Scroll: `--font`, `--color`, `--width` and
`--scroll-rate` are per-element, and by the override rule below they would be
errors on every input `draw` accepts — flags that exist only to be refused. The
right response to an inapplicable flag is not to offer it.
```

And append to §3.3:

```markdown
**Templates take the same override rule as `--file`.** Payload-level flags
(`--priority`, `--led`) override the template's own values when given;
per-element flags (`-x`/`-y`, `--align`, `--screen`, `--timeout`, `--opacity`,
`--id`) are hard errors, because a template may hold several elements with no
principled way to pick which one a single flag applies to. Recorded in
`docs/specs/2026-08-11-phase-4a-templates-design.md` §4.
```

- [ ] **Step 5: Verify and commit**

Run: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

```bash
git add tests/template.rs tests/snapshots docs/
git commit -m "test: golden template payload; docs: correct the inherited template claims"
```

---

## Task 10: README, probe, and the release pass

**Files:**
- Modify: `README.md`, `scripts/probe-device.sh`

- [ ] **Step 1: Update the README**

Add to the example block, after the `busy asset list` line:

```markdown
busy template init                 # write the example templates
busy draw ok                       # a template with no required variables
busy draw error "Build failed"     # a template that requires a message
git log -1 --format=%s | busy draw error -
```

Add `-o`/`--opacity` is already present; add a `--var` row to the short-option table's prose (it is long-only, like the other rarely-typed options).

Add to the notes:

```markdown
- **Templates.** `busy template init` writes examples into
  `~/.config/busy/templates/`; each is a directory with a `template.toml` that
  is the API payload plus a `description`. `busy draw <name>` renders and draws
  one. Variables are minijinja (`{{ message }}`), the positional binds to
  `message`, and `--var k=v` supplies the rest. Every substitution is escaped,
  so a quote in a commit subject is safe.
- **Templates take flags like `--file` does.** `--priority` and `--led`
  override the template's own values; per-element flags (`-x`, `--align`,
  `--opacity`, `--timeout`, `--id`) are errors, because a template may hold
  several elements. Expose anything else as a `{{ variable }}`.
- **Adding an example.** Commit a directory to `templates/` in this repo and
  `busy template init` picks it up — no code change. `tests/examples.rs`
  validates every one, so a broken template fails the build.
```

- [ ] **Step 2: Add a probe assertion for the stock path the examples use**

`templates/error/template.toml` references `shared/cross_front_8x8.image`. Step 8 of `scripts/probe-device.sh` already lists stock assets; add an explicit check after it, matching the script's existing style:

```sh
say "8b. the stock asset the shipped `error` template uses"
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/apps_assets/shared/images" \
  | grep -q 'cross_front_8x8' \
  && echo "  MATCH (shared/cross_front_8x8.image is present)" \
  || echo "  CHANGED -- the error template's icon is gone; pick another"
```

- [ ] **Step 3: Run the full gate**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: all green, roughly 175 tests.

- [ ] **Step 4: Verify against the real bar**

This is the acceptance test for the phase.

```bash
rm -rf ~/.config/busy/templates.bak
mv ~/.config/busy/templates ~/.config/busy/templates.bak 2>/dev/null || true

cargo run -- template init
cargo run -- template list
cargo run -- template validate
cargo run -- draw ok
sleep 2
cargo run -- draw error "Build failed"

# Read the frame back and confirm the text and icon are both on the panel.
curl -s "http://10.0.4.20/api/screen?display=0" -o /tmp/f.b64
python3 -c "
import base64
raw=base64.b64decode(open('/tmp/f.b64').read()); W,H=72,16
for y in range(H): print(''.join('#' if any(raw[(y*W+x)*3:(y*W+x)*3+3]) else '.' for x in range(W)))
"

# The escaping case, on real hardware.
cargo run -- draw error 'He said "hi"'

cargo run -- clear
mv ~/.config/busy/templates.bak ~/.config/busy/templates 2>/dev/null || true
```

The pixel view is the proof: two distinct inked regions (icon at the left, text beside it) rather than one. A template that renders to an empty payload also exits 0, so only the frame distinguishes them. Paste the pixel view and every command's output into the report.

Leave the display cleared.

- [ ] **Step 5: Commit**

```bash
git add README.md scripts/probe-device.sh
git commit -m "docs: README and probe assertions for templates"
```

---

## Definition of done

- `busy template init` writes `error` and `ok`, skips an existing directory, and `--force` replaces it.
- `busy template list | show | validate` work offline and name a near-match on a misspelling.
- `busy draw error "Build failed"` renders and draws; `busy draw ok` works with no arguments.
- `busy draw error 'He said "hi"'` produces valid TOML and the quotes reach the panel.
- A missing required variable, a per-element flag on a template, and `--var` on an image draw are all exit 2 with messages naming the flag or variable.
- A template referencing an absent device asset errors with the exact `busy asset upload` command; a failed listing does not block the draw.
- Adding a directory to `templates/` requires no code change, and `tests/examples.rs` validates it.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` are green.
- A template drawn on the real bar is visible in a frame readback.

## Execution notes, carried from Phases 1–3

**The plan is the likeliest source of defects.** Phase 3 ran nine plan defects against two implementer defects. Do the pre-flight scan before dispatching Task 1: look for a constraint the plan violates itself, and for code the plan mandates that the review rubric would call a defect.

**Have reviewers write reports to a file and return a short verdict.** An inline report costs ~2.5k tokens of permanent context for information whose value drops to zero once acted on.

**Verify by running the binary, not by reading a green suite.** Every serious defect in Phases 1–3 came from execution. For this phase the equivalent is reading a frame back: a template that renders to an empty payload exits 0 and looks identical to one that worked.

**Sonnet is the floor for implementers.**

**Treat an Important finding as a fix round even if the summary says "Approved".**

**Expect API errors mid-task.** Check the working tree before re-dispatching; a dead agent often got further than its last message suggests.

## Deferred, and where it is recorded

**Phase 4b:** content-hash asset sync, `storage/read` comparison, automatic upload of a template's own images — `docs/specs/2026-08-11-phase-4a-templates-design.md` §10.

**Beyond Phase 4:** dedicated `rect`/`countdown` verbs; template sharing via git; `busy status`; stock-asset enumeration for completion.

**Open issues this phase touches:** #7 (`apply_file_overrides` style) is closed by Task 7's `overrides.rs`. #12 (`--until` in `draw --help`) is a prerequisite, not part of this plan. #4 (`--dry-run --json` shape) and #14 (`--screen` dual meaning) remain open and are unaffected.
