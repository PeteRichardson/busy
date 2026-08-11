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
///
/// Read-only in its entirety, so it ignores `--dry-run`: running it *is* the
/// dry run, and `asset delete --dry-run` relies on this exact call to name
/// what it would destroy. Directory entries are included, not filtered out —
/// `delete` counts and destroys them, so `list` must show them too, or a
/// caller relying on "no assets" as a green light for `delete` would be lied
/// to for an app holding only a subdirectory.
pub async fn list(settings: &Settings, emitter: &Emitter) -> Result<(), CliError> {
    let device = Device::connect(settings)?;
    let entries = device.list_assets().await?;

    let mut files: Vec<(String, Option<u64>, bool)> = entries
        .iter()
        .map(|entry| (entry.name().to_owned(), entry.size(), entry.is_dir()))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    if files.is_empty() {
        return emitter.success_list(&format!("no assets for `{}`", settings.app), &files);
    }

    let mut report = String::new();
    for (name, size, is_dir) in &files {
        if *is_dir {
            report.push_str(&format!("{name}/\t0\n"));
        } else {
            report.push_str(&format!("{name}\t{}\n", size.unwrap_or(0)));
        }
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
            "resized {}x{} to {}x{} to fit the {} panel; the bar refuses to draw \
             anything larger",
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

    // `storage/list` is a generic directory browse, so it can in principle
    // return `Dir` entries here too. `delete_assets` removes the whole
    // `/ext/user_assets/<app>/` tree regardless of what's in it, so a
    // directory entry is still something that gets destroyed — it must count
    // toward "is there anything to delete" and appear in the manifest, or the
    // confirmation would understate (or, for an app holding only a
    // subdirectory, entirely miss) what is about to be wiped.
    if entries.is_empty() {
        return emitter.success(&format!("no assets for `{}`", settings.app), None);
    }

    let names: Vec<String> = entries
        .iter()
        .map(|entry| {
            if entry.is_dir() {
                format!("{}/", entry.name())
            } else {
                entry.name().to_owned()
            }
        })
        .collect();

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
        confirm_prompt(&mut std::io::stderr(), emitter, &summary);
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|error| CliError::usage(format!("could not read confirmation: {error}")))?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return emitter.success("cancelled", None);
        }
    } else {
        emitter.warn_always(&summary);
    }

    device.delete_assets().await?;
    emitter.success(&format!("deleted {} asset(s)", names.len()), None)
}

/// Announce the manifest, then the y/N prompt, on `out` — in that order,
/// always. Split out of `delete` so this ordering is regression-testable
/// without a real pty: `delete` only reaches this once `stdin` is a
/// terminal, which the process-spawning integration tests can never provide
/// (their child always gets a piped stdin).
///
/// `Emitter::warn_always` alone does not guarantee the ordering: under
/// `--json` it buffers `summary` for the final JSON document instead of
/// printing it, so without the explicit echo here an interactive `--json`
/// caller would see a bare "Delete them?" and answer blind — exactly the
/// defect this closes. Outside `--json`, `warn_always` already printed the
/// manifest immediately, so the explicit echo is skipped there or the line
/// would double up.
fn confirm_prompt(out: &mut impl std::io::Write, emitter: &Emitter, summary: &str) {
    emitter.warn_always(summary);
    if emitter.json {
        let _ = writeln!(out, "{summary}");
    }
    let _ = write!(out, "Delete them? [y/N] ");
}

#[cfg(test)]
mod tests {
    use super::confirm_prompt;
    use crate::output::Emitter;

    #[test]
    fn confirm_prompt_writes_the_manifest_before_the_prompt_under_json() {
        // The interactive y/N branch can only run against a real terminal,
        // which tests/asset.rs's spawned children never have (piped stdin).
        // This exercises the ordering property directly on the extracted
        // helper instead: the manifest must land before the prompt is
        // asked, in every mode, or a `--json` caller answers blind.
        let emitter = Emitter::new(true, false);
        let mut out = Vec::new();
        confirm_prompt(
            &mut out,
            &emitter,
            "this deletes ALL 1 asset(s) for `busy`: logo.png",
        );
        let written = String::from_utf8(out).expect("valid utf8");
        let manifest_at = written
            .find("logo.png")
            .expect("the manifest must be written to the prompt stream");
        let prompt_at = written
            .find("Delete them?")
            .expect("the prompt must be written");
        assert!(
            manifest_at < prompt_at,
            "manifest must precede the prompt, got: {written:?}"
        );
    }

    #[test]
    fn confirm_prompt_does_not_double_the_manifest_outside_json() {
        // Outside --json, `Emitter::warn_always` already prints the manifest
        // immediately to the real stderr. The explicit writer here must
        // carry only the prompt, or a human sees the file list twice.
        let emitter = Emitter::new(false, false);
        let mut out = Vec::new();
        confirm_prompt(
            &mut out,
            &emitter,
            "this deletes ALL 1 asset(s) for `busy`: logo.png",
        );
        let written = String::from_utf8(out).expect("valid utf8");
        assert!(
            !written.contains("logo.png"),
            "must not duplicate the manifest outside --json, got: {written:?}"
        );
        assert!(written.contains("Delete them?"));
    }
}
