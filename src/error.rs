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
