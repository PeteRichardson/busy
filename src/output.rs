//! Human, `--json`, and `--dry-run` emission.

use std::cell::RefCell;

use crate::device::DisplayElements;
use crate::error::CliError;

/// Under `--json`, warnings are buffered here instead of being printed
/// immediately, so a run that both warns and fails still produces exactly
/// one parseable JSON document on stderr rather than a prose line followed
/// by one. Interior mutability lets every command hold `&Emitter` rather
/// than threading a `&mut` through `main`, `cmd::text`, and `cmd::clear`.
#[derive(Debug)]
pub struct Emitter {
    pub json: bool,
    pub quiet: bool,
    warnings: RefCell<Vec<String>>,
}

impl Emitter {
    pub fn new(json: bool, quiet: bool) -> Self {
        Self {
            json,
            quiet,
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Report a warning. `--quiet` suppresses it entirely, matching the
    /// flag's own help text ("Suppress warnings"). Otherwise: under
    /// `--json` it is buffered so `success`/`failure`/`dry_run` can fold it
    /// into the single JSON document they emit; without `--json` it prints
    /// immediately, as before.
    pub fn warn(&self, message: &str) {
        if self.quiet {
            return;
        }
        self.warn_always(message);
    }

    /// Like `warn`, but ignores `--quiet`. For warnings that mean "your
    /// configuration is not being applied" — a malformed config file is
    /// discarded wholesale — which is categorically different from a
    /// routine advisory and must not go silent just because the caller
    /// wanted quiet output.
    pub fn warn_always(&self, message: &str) {
        if self.json {
            self.warnings.borrow_mut().push(message.to_owned());
        } else {
            eprintln!("busy: warning: {message}");
        }
    }

    /// Print the exact bytes that would be sent, and nothing else on
    /// stdout — pinned by `dry_run_output_is_unaffected_by_json`. Any
    /// warnings buffered under `--json` still need to reach the caller, so
    /// they are flushed as their own JSON object on stderr rather than
    /// silently dropped.
    pub fn dry_run(&self, payload: &DisplayElements) -> Result<(), CliError> {
        let json = serde_json::to_string_pretty(payload)
            .map_err(|error| CliError::runtime(format!("could not serialize payload: {error}")))?;
        println!("{json}");
        self.flush_warnings();
        Ok(())
    }

    /// Report success. `payload` is the wire body when the command sent one
    /// (e.g. `text`); commands with nothing to echo back (e.g. `clear`) pass
    /// `None` and the `"payload"` key is simply omitted, so every command's
    /// `--json` output shares one envelope rather than each inventing its own.
    pub fn success(
        &self,
        summary: &str,
        payload: Option<&DisplayElements>,
    ) -> Result<(), CliError> {
        if self.json {
            let mut body = serde_json::json!({
                "ok": true,
                "summary": summary,
            });
            if let Some(payload) = payload {
                let payload = serde_json::to_value(payload).map_err(|error| {
                    CliError::runtime(format!("could not serialize output: {error}"))
                })?;
                body["payload"] = payload;
            }
            self.attach_warnings(&mut body);
            let json = serde_json::to_string_pretty(&body).map_err(|error| {
                CliError::runtime(format!("could not serialize output: {error}"))
            })?;
            println!("{json}");
        } else if !self.quiet {
            println!("{summary}");
        }
        Ok(())
    }

    /// Report success for a listing whose items a machine consumer needs
    /// individually addressable rather than folded into one formatted blob.
    /// `summary` is the human-facing text printed as-is without `--json`
    /// (a `name\tsize` block followed by a count line); under `--json` that
    /// text is dropped in favour of an `"assets"` array of `{name, size}`
    /// objects, so a script never has to re-parse tabs and newlines out of a
    /// JSON string to get at the data — the same class of "technically
    /// valid but useless" JSON that warnings-during-success produced before
    /// they were buffered (see `warn_always`).
    pub fn success_list(&self, summary: &str, items: &[(&str, u64)]) -> Result<(), CliError> {
        if self.json {
            let assets: Vec<_> = items
                .iter()
                .map(|(name, size)| serde_json::json!({"name": name, "size": size}))
                .collect();
            let mut body = serde_json::json!({
                "ok": true,
                "summary": format!("{} asset(s)", items.len()),
                "assets": assets,
            });
            self.attach_warnings(&mut body);
            let json = serde_json::to_string_pretty(&body).map_err(|error| {
                CliError::runtime(format!("could not serialize output: {error}"))
            })?;
            println!("{json}");
        } else if !self.quiet {
            println!("{summary}");
        }
        Ok(())
    }

    /// Report a failure. Under `--json` this writes a parseable object to
    /// stderr so a wrapper script can branch on it without scraping prose —
    /// any warnings buffered earlier in the run ride along in the same
    /// object instead of being printed ahead of it, which is what made the
    /// stream unparseable before.
    pub fn failure(&self, error: &CliError) {
        if self.json {
            let mut body = serde_json::json!({
                "ok": false,
                "error": error.to_string(),
            });
            self.attach_warnings(&mut body);
            match serde_json::to_string_pretty(&body) {
                Ok(json) => eprintln!("{json}"),
                Err(_) => eprintln!("busy: {error}"),
            }
        } else {
            eprintln!("busy: {error}");
        }
    }

    /// Drain the buffered warnings into `body["warnings"]`, omitting the key
    /// entirely when there are none so existing consumers of a warning-free
    /// response see no shape change.
    fn attach_warnings(&self, body: &mut serde_json::Value) {
        let warnings = self.warnings.borrow_mut().split_off(0);
        if !warnings.is_empty() {
            body["warnings"] = serde_json::json!(warnings);
        }
    }

    /// Drain and print any buffered `--json` warnings as their own JSON
    /// object on stderr. A no-op when there is nothing buffered, or when
    /// `--json` is off (in which case `warn` already printed immediately).
    fn flush_warnings(&self) {
        if !self.json {
            return;
        }
        let warnings = self.warnings.borrow_mut().split_off(0);
        if warnings.is_empty() {
            return;
        }
        let body = serde_json::json!({ "warnings": warnings });
        if let Ok(json) = serde_json::to_string_pretty(&body) {
            eprintln!("{json}");
        }
    }
}
