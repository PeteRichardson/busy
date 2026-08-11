//! Rendering a template's text.
//!
//! The only module that imports `minijinja`, for the same reason `device.rs`
//! is the only one importing `busylib`: one file to fix when an upstream
//! layout moves.
//!
//! Substitution happens over the raw TOML text, before it is parsed, which is
//! what lets a variable appear in any field. That also means an unescaped
//! quote in a value would corrupt the document — and `message` routinely
//! arrives from `git log -1 --format=%s`. So every substitution is escaped for
//! a TOML basic string unless the template explicitly asks otherwise.

use std::collections::BTreeMap;

use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};

use crate::error::CliError;

/// Constructs whose targets `undeclared_variables` does not follow. A template
/// using one would under-report its required variables, so they are refused.
const FORBIDDEN: [&str; 4] = ["include", "import", "from", "extends"];

/// Escape for a TOML basic string. Digits, letters, and ordinary punctuation
/// pass through, so a numeric field is unaffected.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_name| AutoEscape::Custom("toml"));
    env.set_formatter(|out, state, value: &Value| {
        // `is_safe()` is what makes `| safe` mean anything. Without this check
        // the filter is silently ignored — verified by running it.
        if state.auto_escape() == AutoEscape::Custom("toml") && !value.is_safe() {
            out.write_str(&escape(&value.to_string()))?;
            return Ok(());
        }
        minijinja::escape_formatter(out, state, value)
    });
    env
}

/// Refuse the constructs `undeclared_variables` cannot see through.
fn reject_forbidden(name: &str, source: &str) -> Result<(), CliError> {
    for keyword in FORBIDDEN {
        let needle = format!("{{% {keyword}");
        let compact = format!("{{%{keyword}");
        if source.contains(&needle) || source.contains(&compact) {
            return Err(CliError::usage(format!(
                "template `{name}` uses `{{% {keyword} %}}`, which is not supported: \
                 templates must be self-contained single files, because the analysis \
                 that reports a template's required variables cannot see through it."
            )));
        }
    }
    Ok(())
}

/// The variables `source` references, sorted. Static analysis, so it runs
/// before rendering and produces a real error rather than a render failure.
///
/// Over-reports rather than under-reports: a variable mentioned only inside a
/// never-taken branch is still listed. `template show` says so.
// Not yet called outside tests — the commands that consume it land in later
// tasks of this phase. `cfg_attr(not(test), ...)` keeps the expectation
// accurate under both `cargo test` and `cargo clippy --all-targets`; once real
// callers exist, drop this and let dead-code analysis run normally.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn analyse(name: &str, source: &str) -> Result<Vec<String>, CliError> {
    reject_forbidden(name, source)?;

    let mut env = environment();
    // `add_template_owned` accepts owned `String`s directly (they satisfy
    // `Into<Cow<'source, str>>`), so the environment holds its own copies and
    // drops them when it goes out of scope at the end of this function.
    // `add_template` would instead demand `&'source str` borrows that outlive
    // the `Environment`, which for text read from a file at runtime would
    // force either leaking or restructuring every caller around a lifetime.
    // `validate` (Task 5) calls `analyse` once per installed template in a
    // loop, so avoiding a per-call leak here keeps that loop's memory bounded
    // by "one template alive at a time" rather than "every template ever
    // analysed this run".
    env.add_template_owned(name.to_owned(), source.to_owned())
        .map_err(|error| syntax_error(name, &error))?;
    let template = env
        .get_template(name)
        .map_err(|error| syntax_error(name, &error))?;

    let mut found: Vec<String> = template.undeclared_variables(false).into_iter().collect();
    found.sort();
    Ok(found)
}

/// Render `source` with `vars`, escaping every substitution.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn render(
    name: &str,
    source: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, CliError> {
    reject_forbidden(name, source)?;

    let mut env = environment();
    env.add_template_owned(name.to_owned(), source.to_owned())
        .map_err(|error| syntax_error(name, &error))?;
    let template = env
        .get_template(name)
        .map_err(|error| syntax_error(name, &error))?;

    template
        .render(vars)
        .map_err(|error| syntax_error(name, &error))
}

fn syntax_error(name: &str, error: &minijinja::Error) -> CliError {
    let mut message = format!("template `{name}`: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    CliError::usage(message)
}

#[cfg(test)]
mod tests {
    use super::{analyse, render};
    use std::collections::BTreeMap;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_quote_in_a_value_cannot_break_the_document() {
        // The whole reason auto-escaping exists: `message` routinely arrives
        // from `git log -1 --format=%s`, and a commit subject may contain
        // anything. Verified against minijinja before this plan was written.
        let out = render(
            "t",
            r#"text = "{{ message }}""#,
            &vars(&[("message", "He said \"hi\"")]),
        )
        .expect("should render");
        assert_eq!(out, r#"text = "He said \"hi\"""#);
        toml::from_str::<toml::Value>(&out).expect("escaped output must be valid TOML");
    }

    #[test]
    fn backslashes_and_control_characters_are_escaped() {
        let out = render(
            "t",
            r#"text = "{{ message }}""#,
            &vars(&[("message", "a\\b\nc\td")]),
        )
        .expect("should render");
        assert_eq!(out, r#"text = "a\\b\nc\td""#);
        toml::from_str::<toml::Value>(&out).expect("must be valid TOML");
    }

    #[test]
    fn a_numeric_field_passes_through_untouched() {
        // Escaping must not quote or alter a value used in a non-string
        // field, or `x = {{ pos }}` stops working.
        let out = render("t", "x = {{ pos }}", &vars(&[("pos", "4")])).expect("should render");
        assert_eq!(out, "x = 4");
    }

    #[test]
    fn the_safe_filter_opts_out_of_escaping() {
        // Verified: without an `is_safe()` check in the formatter this silently
        // escapes anyway, which would make `| safe` a lie.
        let out = render(
            "t",
            r#"raw = {{ value | safe }}"#,
            &vars(&[("value", "[1, 2]")]),
        )
        .expect("should render");
        assert_eq!(out, "raw = [1, 2]");
    }

    #[test]
    fn a_missing_variable_is_an_error_not_an_empty_string() {
        // Strict mode. Rendering `text = ""` would hit the device's
        // minLength: 1 as a confusing 400 instead of a local error.
        let error = render("greet", r#"text = "{{ message }}""#, &vars(&[]))
            .expect_err("should fail")
            .to_string();
        assert!(
            error.contains("greet"),
            "should name the template, got {error}"
        );
    }

    #[test]
    fn analyse_reports_the_variables_a_template_references() {
        let found = analyse(
            "t",
            r#"text = "{{ message }}"
x = {{ pos }}"#,
        )
        .expect("should analyse");
        assert_eq!(found, vec!["message".to_owned(), "pos".to_owned()]);
    }

    #[test]
    fn inheritance_constructs_are_rejected() {
        // `undeclared_variables` does not follow these, so a template using
        // one would silently under-report what it needs.
        for bad in [
            "{% include 'other.toml' %}",
            "{% import 'x.toml' as x %}",
            "{% from 'x.toml' import y %}",
            "{% extends 'base.toml' %}",
        ] {
            let error = analyse("t", bad)
                .expect_err(&format!("{bad} should be rejected"))
                .to_string();
            assert!(
                error.contains("self-contained"),
                "should explain why, got {error}"
            );
        }
    }

    #[test]
    fn a_syntax_error_names_the_template() {
        let error = analyse("broken", "text = \"{{ unclosed \"")
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("broken"), "got {error}");
    }
}
