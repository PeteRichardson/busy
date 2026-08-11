//! `busy template` — manage and run templates.

use include_dir::{Dir, include_dir};

use crate::cli::{TemplateInitArgs, TemplateShowArgs, TemplateValidateArgs};
use crate::config::Settings;
use crate::error::CliError;
use crate::output::Emitter;
use crate::template::{self, Template, discover, validate};

/// The warning text shown when a variable value needed transliterating to
/// printable ASCII before it could be substituted into a template. Deliberately
/// close to `Command::Text`'s own wording (`src/main.rs`) — same problem, same
/// explanation — but the subject is a template variable rather than "the
/// message", since here it is one or more `--var`/placeholder values rather
/// than a single positional argument.
const SANITIZED_VARIABLE_WARNING: &str = "one or more template variable values contained \
     characters the bar's bitmap-ASCII fonts cannot render (smart quotes, dashes, or similar) \
     and were transliterated to plain ASCII";

/// The template root, or a usage error explaining that there is no config
/// directory to put one in.
pub fn root(flag: Option<&std::path::Path>) -> Result<std::path::PathBuf, CliError> {
    discover::root(flag).ok_or_else(|| {
        CliError::usage(
            "could not determine a config directory for templates; pass --template-dir.",
        )
    })
}

pub fn list(root: &std::path::Path, emitter: &Emitter) -> Result<(), CliError> {
    let names = discover::list(root);

    // One pass gathering (name, description) pairs, then both the human
    // report and the `--json` array are built from it — reading each
    // template's description only once, not once per output format.
    let entries: Vec<(String, String)> = names
        .iter()
        .map(|name| {
            let description = Template::load(root, name)
                .ok()
                .and_then(|template| {
                    toml::from_str::<crate::template::TemplateFile>(&template.source)
                        .ok()
                        .and_then(|file| file.description)
                })
                .unwrap_or_default();
            (name.clone(), description)
        })
        .collect();

    // `{name, description}` objects, addressable individually, rather than
    // folding the listing into one formatted blob under `--json` — the
    // defect `success_items` (nee `success_list`, generalized for this)
    // exists to prevent. See its doc comment.
    let templates: Vec<serde_json::Value> = entries
        .iter()
        .map(|(name, description)| serde_json::json!({ "name": name, "description": description }))
        .collect();
    let json_summary = format!("{} template(s)", entries.len());

    if entries.is_empty() {
        return emitter.success_items(
            &format!(
                "no templates in {}; run `busy template init` to write the examples",
                root.display()
            ),
            &json_summary,
            "templates",
            templates,
            true,
        );
    }

    let mut report = String::new();
    for (name, description) in &entries {
        report.push_str(&format!("{name}\t{description}\n"));
    }
    report.push_str(&format!("{} template(s)", entries.len()));
    emitter.success_items(&report, &json_summary, "templates", templates, true)
}

pub fn show(
    args: &TemplateShowArgs,
    root: &std::path::Path,
    emitter: &Emitter,
) -> Result<(), CliError> {
    let template = Template::load(root, &args.name)?;
    let file: crate::template::TemplateFile = toml::from_str(&template.source)
        .map_err(|error| CliError::usage(format!("template `{}`: {error}", args.name)))?;
    let variables = template.required_variables()?;

    let mut report = format!("{}\n", args.name);
    if let Some(description) = &file.description {
        report.push_str(&format!("  {description}\n"));
    }
    report.push_str(&format!("  {} element(s)\n", file.elements.len()));
    if variables.is_empty() {
        report.push_str("  no variables\n");
    } else {
        // Static analysis over-reports: a variable mentioned only inside a
        // never-taken branch is still listed. Say so rather than implying
        // every one is mandatory.
        report.push_str(&format!(
            "  variables mentioned: {}\n",
            variables.join(", ")
        ));
    }
    report.push_str(&format!("  {}", template.dir.display()));

    // Structured fields under `--json` — `name`, `description`, `elements`
    // (a count), `variables` (an array), `path` — rather than the human
    // report's formatted lines folded into one string. The human text above
    // is unchanged either way.
    emitter.success_fields(
        &report,
        serde_json::json!({
            "name": args.name,
            "description": file.description,
            "elements": file.elements.len(),
            "variables": variables,
            "path": template.dir.display().to_string(),
        }),
        true,
    )
}

pub fn validate(
    args: &TemplateValidateArgs,
    root: &std::path::Path,
    settings: &Settings,
    emitter: &Emitter,
) -> Result<(), CliError> {
    let names = match &args.name {
        Some(name) => vec![name.clone()],
        None => discover::list(root),
    };

    if names.is_empty() {
        return emitter.success("no templates to validate", None);
    }

    let mut failures = Vec::new();
    let mut sanitized_anything = false;
    for name in &names {
        let template = Template::load(root, name)?;

        // Bind every referenced variable to a placeholder so the render can
        // proceed: validation is about the template's shape, not about whether
        // the caller happened to supply values.
        let variables = template.required_variables()?;
        let mut vars = variables
            .into_iter()
            .map(|key| (key, "x".to_owned()))
            .collect();

        // `bind_variables` is not in play here — there is no `k=v` pair to
        // parse, just one synthetic placeholder per required variable — so
        // this path must sanitize on its own to keep the same "every render
        // sees ASCII-safe values" guarantee `bind_variables` gives `--var`
        // and the positional. See `template::sanitize_values`'s doc comment.
        if template::sanitize_values(&mut vars) {
            sanitized_anything = true;
        }

        let file = template.render(&vars)?;
        let payload = file.into_payload(&settings.app)?;
        let report = validate::offline(&payload, &template.dir);

        for warning in &report.warnings {
            emitter.warn(&format!("{name}: {warning}"));
        }
        for error in &report.errors {
            failures.push(format!("{name}: {error}"));
        }
    }

    // Once per invocation, not once per variable and not once per template:
    // several templates each needing a substitution sanitized is still one
    // fact ("something in this run needed transliterating"), not N facts.
    if sanitized_anything {
        emitter.warn(SANITIZED_VARIABLE_WARNING);
    }

    if !failures.is_empty() {
        return Err(CliError::usage(failures.join("\n")));
    }
    emitter.success(&format!("{} template(s) OK", names.len()), None)
}

/// The shipped example templates, embedded at compile time.
///
/// Adding an example is a commit to `templates/`, not a code change: `init`
/// walks this tree rather than a hand-maintained list. Whole directories are
/// embedded rather than single files, so a future example carrying a PNG works
/// with no change here.
static EXAMPLES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// Write the shipped example templates into `root`.
///
/// Each example is its own directory (`error/`, `ok/`, …) under `EXAMPLES`;
/// one that already exists on disk is left alone unless `--force` is given,
/// so re-running `init` after hand-editing an example never clobbers it.
pub fn init(
    args: &TemplateInitArgs,
    root: &std::path::Path,
    emitter: &Emitter,
) -> Result<(), CliError> {
    std::fs::create_dir_all(root).map_err(|error| {
        CliError::runtime(format!("could not create {}: {error}", root.display()))
    })?;

    let mut written = Vec::new();
    let mut skipped = Vec::new();

    for dir in EXAMPLES.dirs() {
        let Some(name) = dir.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `get_file` wants the path as stored, which includes the directory
        // prefix — `get_file("template.toml")` returns None.
        if dir.get_file(dir.path().join("template.toml")).is_none() {
            continue;
        }

        let destination = root.join(name);
        if destination.exists() && !args.force {
            skipped.push(name.to_owned());
            continue;
        }

        std::fs::create_dir_all(&destination).map_err(|error| {
            CliError::runtime(format!(
                "could not create {}: {error}",
                destination.display()
            ))
        })?;

        for file in dir.files() {
            let Some(leaf) = file.path().file_name() else {
                continue;
            };
            let target = destination.join(leaf);
            std::fs::write(&target, file.contents()).map_err(|error| {
                CliError::runtime(format!("could not write {}: {error}", target.display()))
            })?;
        }
        written.push(name.to_owned());
    }

    // Human text stays prose; `--json` gets `written`/`skipped` as their own
    // addressable arrays rather than folding both into one formatted
    // `summary` blob — the shape Task 5 (`template list`/`show`) fixed twice
    // over (see `Emitter::success_items`'s doc comment). A script that wants
    // to know exactly which examples arrived or were kept can read the
    // arrays directly instead of parsing sentences back out of a string.
    let mut report = String::new();
    if !written.is_empty() {
        report.push_str(&format!(
            "wrote {} to {}\n",
            written.join(", "),
            root.display()
        ));
    }
    if !skipped.is_empty() {
        report.push_str(&format!(
            "kept your existing {} (use --force to replace)\n",
            skipped.join(", ")
        ));
    }
    if report.is_empty() {
        report.push_str("no examples are bundled with this build");
    }

    emitter.success_fields(
        report.trim_end(),
        serde_json::json!({
            "written": written,
            "skipped": skipped,
        }),
        true,
    )
}
