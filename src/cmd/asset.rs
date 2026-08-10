//! `busy asset` — upload, list, and delete this application's assets.

use crate::cli::AssetUploadArgs;
use crate::config::{self, Settings};
use crate::device::{AssetName, Device, Screen};
use crate::error::CliError;
use crate::image;
use crate::output::Emitter;

/// Convert a local image to a panel-sized PNG and upload it.
///
/// Conversion happens here rather than at draw time because `assets/upload` is
/// a dumb byte write: it accepts a JPEG happily and the failure only surfaces
/// later, from a different command, as a device error about an `/ext` path.
pub async fn upload(
    args: &AssetUploadArgs,
    settings: &Settings,
    emitter: &Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    let bytes = std::fs::read(&args.path).map_err(|error| {
        CliError::usage(format!("could not read {}: {error}", args.path.display()))
    })?;

    let screen = args
        .screen
        .map(config::screen_from_arg)
        .unwrap_or(settings.screen);
    let target = config::Defaults::panel(screen);

    let prepared = image::prepare(&bytes, target)?;

    let stem = args
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            CliError::usage(format!("{} has no usable file name", args.path.display()))
        })?;
    let name = format!("{stem}.png");
    let name = AssetName::new(name.clone()).map_err(|error| {
        CliError::usage(format!(
            "`{name}` is not a usable asset name: {error}. Rename the file to use only \
             letters, digits, dot, underscore, or hyphen."
        ))
    })?;

    if prepared.was_resized() {
        emitter.warn(&format!(
            "resized {}x{} to {}x{} to fit the {} panel; the bar crops anything larger \
             without saying so",
            prepared.original.0,
            prepared.original.1,
            prepared.final_size.0,
            prepared.final_size.1,
            match screen {
                Screen::Front => "front",
                Screen::Back => "back",
            }
        ));
    }

    let original_name = args.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if original_name != name.as_str() {
        emitter.warn(&format!(
            "stored as `{name}`: the bar only decodes PNG, so `{original_name}` was \
             re-encoded"
        ));
    }

    if dry_run {
        return emitter.success(
            &format!(
                "would upload {} bytes as `{name}` ({}x{})",
                prepared.png.len(),
                prepared.final_size.0,
                prepared.final_size.1
            ),
            None,
        );
    }

    let device = Device::connect(settings)?;
    device.upload(name.as_str(), prepared.png).await?;

    emitter.success(&format!("uploaded `{name}`"), None)
}
