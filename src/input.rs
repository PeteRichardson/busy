//! Where a message comes from.

use crate::error::CliError;

/// Resolve the message argument. `-` means stdin, which is how CI usually has
/// the text already: `git log -1 --format=%s | busy text -`. The same
/// sentinel works for `busy draw`/`busy template run`'s message positional
/// (command-surface spec §2.3) — `busy draw`'s caller in `cmd::draw::run`
/// calls this unconditionally for any `Some` message, and this function is a
/// no-op for anything that is not literally `"-"`.
///
/// A single-character `-` message is not reachable: `busy text -- -` still
/// hits this sentinel, because this function only ever sees the resolved
/// string value and has no way to know whether `--` preceded it on the
/// command line. Any other message starting with `-` (e.g. `-3 tests
/// failing`) is unaffected and only needs `--` to stop clap from treating it
/// as a flag.
pub fn read_message(argument: &str) -> Result<String, CliError> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }
    read_from(&mut std::io::stdin())
}

/// The stdin-reading half of `read_message`, split out so the I/O-failure
/// path is unit-testable without a real, broken stdin — the same reasoning
/// `cmd::asset::read_confirmation` was split out for.
fn read_from(reader: &mut impl std::io::Read) -> Result<String, CliError> {
    let mut buffer = String::new();
    // A failed read is the environment's fault (the terminal going away, a
    // broken pipe), not the user typing something wrong — `runtime`, exit 1.
    // This was mislabelled `usage` before the message path was wired to
    // anything but `busy text -`; see `src/cmd/asset.rs::read_confirmation`
    // for the identical reasoning applied to the confirmation prompt's read.
    reader.read_to_string(&mut buffer).map_err(|error| {
        CliError::runtime(format!("could not read the message from stdin: {error}"))
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
        // Generic on purpose: `-` reads stdin on `busy text`, `busy draw`,
        // and `busy template run` alike (see this function's doc comment),
        // so this must not name whichever one happened to be first.
        return Err(CliError::usage(
            "stdin was empty; `-` expects a message on stdin",
        ));
    }

    Ok(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::read_from;
    use crate::error::CliError;

    /// A `Read` whose every read fails, standing in for a real stdin that has
    /// gone away (closed terminal, broken pipe, and so on).
    struct FailingReader;

    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("no such device"))
        }
    }

    #[test]
    fn a_failed_stdin_read_is_a_runtime_error_not_a_usage_error() {
        // This was `CliError::Usage` (exit 2) before this fix round, which
        // mislabelled an I/O failure as the user's mistake. It went untested
        // because no CLI path fed a real `-` into anything but `busy text -`
        // until `draw`/`template run` were wired up — see the fix report.
        let error = read_from(&mut FailingReader).expect_err("should fail");
        assert!(
            matches!(error, CliError::Runtime(_)),
            "a stdin read failure must be a runtime error, not a usage error; got {error:?}"
        );
    }

    #[test]
    fn empty_stdin_is_still_a_usage_error() {
        // The I/O read succeeding with nothing on it is the caller's mistake
        // (piped nothing, or an interactive terminal with no input), unlike
        // the read itself failing above — distinct causes, distinct kinds.
        let error = read_from(&mut std::io::empty()).expect_err("should fail");
        assert!(
            matches!(error, CliError::Usage(_)),
            "empty stdin must stay a usage error; got {error:?}"
        );
    }
}
