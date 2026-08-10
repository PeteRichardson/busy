//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

// `busy-cli` is a binary crate, so these `pub use` re-exports are not public
// API in the way they would be for a library: an item with no in-crate
// consumer is just an unused import as far as clippy is concerned. This
// module is a deliberate façade that concentrates all `busylib` coupling in
// one file, so its re-export list is written for the whole crate up front
// and may briefly outrun its first consumer as later tasks land. Silence
// that lag here rather than trimming the list or reaching for a
// crate-level allow.
#[allow(unused_imports)]
pub use busylib::model::assets::{
    Align, DisplayElement, DisplayElements, ElementKind, Font, Lifetime, Screen, TextElement,
};
#[allow(unused_imports)]
pub use busylib::types::app_name::AppName;
pub use busylib::types::color::Color;
#[allow(unused_imports)]
pub use busylib::types::element_id::ElementId;
#[allow(unused_imports)]
pub use busylib::types::priority::Priority;
#[allow(unused_imports)]
pub use busylib::types::text::Text;

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

    CliError::runtime(chain(&error))
}

/// `busylib::Error`'s own `Display` names the request but not the underlying
/// cause — for `an_unreachable_device_fails_with_exit_1` that's the
/// difference between "unable to reach device" and a message that actually
/// contains the address that could not be reached. Walk the `source()` chain
/// so the low-level cause (a refused TCP connection, DNS failure, and so on)
/// reaches the user too.
fn chain(error: &busylib::Error) -> String {
    use std::error::Error as _;

    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    message
}
