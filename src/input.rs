//! Where a message comes from.

use std::io::Read as _;

use crate::error::CliError;

/// Resolve the message argument. `-` means stdin, which is how CI usually has
/// the text already: `git log -1 --format=%s | busy text -`.
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

    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| {
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
