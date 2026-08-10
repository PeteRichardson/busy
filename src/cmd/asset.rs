//! `busy asset` — upload, list, and delete this application's assets.

use std::io::IsTerminal as _;

use crate::cli::{AssetDeleteArgs, AssetUploadArgs};
use crate::config::{self, Settings};
use crate::device::{AssetName, Device, Screen};
use crate::error::CliError;
use crate::image;
use crate::output::Emitter;

/// List this application's assets, read from the device rather than from any
/// local record — there is no local record, deliberately.
pub async fn list(settings: &Settings, emitter: &Emitter, dry_run: bool) -> Result<(), CliError> {
    if dry_run {
        return emitter.success(&format!("would list assets for `{}`", settings.app), None);
    }

    let device = Device::connect(settings)?;
    let entries = device.list_assets().await?;

    let mut files: Vec<(&str, u64)> = entries
        .iter()
        .filter(|entry| !entry.is_dir())
        .map(|entry| (entry.name(), entry.size().unwrap_or(0)))
        .collect();
    files.sort_by_key(|(name, _)| *name);

    if files.is_empty() {
        return emitter.success_list(&format!("no assets for `{}`", settings.app), &files);
    }

    let mut report = String::new();
    for (name, size) in &files {
        report.push_str(&format!("{name}\t{size}\n"));
    }
    report.push_str(&format!("{} asset(s)", files.len()));

    emitter.success_list(&report, &files)
}

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

/// Delete every asset belonging to this application.
///
/// The API has no per-file delete — `storage/remove` returns 400 on a real
/// asset path and the file survives — so this is all-or-nothing, and the file
/// list is printed first to make the blast radius concrete.
pub async fn delete(
    args: &AssetDeleteArgs,
    settings: &Settings,
    emitter: &Emitter,
    dry_run: bool,
) -> Result<(), CliError> {
    let device = Device::connect(settings)?;
    let entries = device.list_assets().await?;
    let names: Vec<&str> = entries
        .iter()
        .filter(|entry| !entry.is_dir())
        .map(|entry| entry.name())
        .collect();

    if names.is_empty() {
        return emitter.success(&format!("no assets for `{}`", settings.app), None);
    }

    let summary = format!(
        "this deletes ALL {} asset(s) for `{}`: {}",
        names.len(),
        settings.app,
        names.join(", ")
    );

    if dry_run {
        return emitter.success(&format!("would delete: {summary}"), None);
    }

    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(CliError::usage(format!(
                "{summary}\nRefusing to delete without confirmation. Re-run with --yes."
            )));
        }
        emitter.warn_always(&summary);
        eprint!("Delete them? [y/N] ");
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|error| CliError::usage(format!("could not read confirmation: {error}")))?;
        if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes") {
            return emitter.success("cancelled", None);
        }
    } else {
        emitter.warn_always(&summary);
    }

    device.delete_assets().await?;
    emitter.success(&format!("deleted {} asset(s)", names.len()), None)
}
