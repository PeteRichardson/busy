//! `busy template` — manage and run templates.

use crate::cli::{TemplateShowArgs, TemplateValidateArgs};
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
    if names.is_empty() {
        return emitter.success(
            &format!(
                "no templates in {}; run `busy template init` to write the examples",
                root.display()
            ),
            None,
        );
    }

    let mut report = String::new();
    for name in &names {
        let description = Template::load(root, name)
            .ok()
            .and_then(|template| {
                toml::from_str::<crate::template::TemplateFile>(&template.source)
                    .ok()
                    .and_then(|file| file.description)
            })
            .unwrap_or_default();
        report.push_str(&format!("{name}\t{description}\n"));
    }
    report.push_str(&format!("{} template(s)", names.len()));
    emitter.success(&report, None)
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
    emitter.success(&report, None)
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
