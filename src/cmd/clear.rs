//! `busy clear` — remove everything this application has drawn.
//!
//! Scoped to `application_name`, so it never disturbs another app's elements.

use crate::device::Device;
use crate::error::CliError;
use crate::output::Emitter;

pub async fn run(
    device: &Device,
    app: &str,
    emitter: Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    if dry_run {
        println!("would clear all elements drawn by `{app}`");
        return Ok(());
    }

    device.clear().await?;

    emitter.success("cleared", None)
}
