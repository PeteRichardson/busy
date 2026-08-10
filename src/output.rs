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
