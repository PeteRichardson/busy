//! `busy clear` — remove everything this application has drawn.
//!
//! Scoped to `application_name`, so it never disturbs another app's elements.

use crate::config::Settings;
use crate::device::Device;
use crate::error::CliError;
use crate::output::Emitter;

pub async fn run(settings: &Settings, emitter: &Emitter, dry_run: bool) -> Result<(), CliError> {
    if dry_run {
        // Checked before `Device::connect` so `--dry-run` is the same
        // "contacts nothing, validates nothing external" escape hatch for
        // `clear` that it is for `text`, and routed through the same
        // `Emitter` so `--json` and `--quiet` behave identically too.
        return emitter.success(
            &format!("would clear all elements drawn by `{}`", settings.app),
            None,
        );
    }

    let device = Device::connect(settings)?;
    device.clear().await?;

    emitter.success("cleared", None)
}
