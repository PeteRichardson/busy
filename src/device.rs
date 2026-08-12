//! The only module that names `busylib`.
//!
//! Everything else imports the model and value types from here, so an upstream
//! module reshuffle is a one-file fix rather than a shotgun edit.

pub use busylib::model::assets::{
    Align, AnimationElement, DisplayElement, DisplayElements, ElementKind, Font, ImageElement,
    ImageSource, Lifetime, Screen, TextElement,
};
pub use busylib::model::storage::StorageListElement;
pub use busylib::types::app_name::AppName;
pub use busylib::types::asset_name::AssetName;
pub use busylib::types::asset_path::AssetPath;
pub use busylib::types::color::Color;
pub use busylib::types::element_id::ElementId;
pub use busylib::types::opacity::Opacity;
pub use busylib::types::priority::Priority;
pub use busylib::types::stock_path::StockPath;
pub use busylib::types::text::Text;

use std::time::Duration;

use busylib::types::storage_path::StoragePath;
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

    /// Write one asset into this application's directory. Overwrites in place.
    pub async fn upload(&self, file: &str, bytes: Vec<u8>) -> Result<(), CliError> {
        self.client
            .assets()
            .upload(self.app.clone(), file, bytes)
            .await
            .map_err(|error| map_error(error, None))
    }

    /// This application's assets, newest listing from the device itself.
    ///
    /// App assets live at `/ext/user_assets/<application_name>/` — undocumented,
    /// learned from the text of a 400. `DELETE assets/upload` removes the
    /// directory rather than emptying it, so a 400 here means "no assets"
    /// rather than a failure.
    pub async fn list_assets(&self) -> Result<Vec<StorageListElement>, CliError> {
        let path = format!("/ext/user_assets/{}", self.app);
        let path = StoragePath::new(path)
            .map_err(|error| CliError::runtime(format!("invalid asset path: {error}")))?;

        match self.client.storage().list(path).await {
            Ok(entries) => Ok(entries),
            Err(error) if error.is_status(StatusCode::BAD_REQUEST) => Ok(Vec::new()),
            Err(error) => Err(map_error(error, None)),
        }
    }

    /// Delete every asset belonging to this application.
    ///
    /// All-or-nothing: the API offers no per-file delete. `storage/remove`
    /// returns 400 on a real asset path and the file survives — measured.
    pub async fn delete_assets(&self) -> Result<(), CliError> {
        self.client
            .assets()
            .delete(self.app.clone())
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
