//! Templates: a directory holding a `template.toml` that renders to a payload.

pub mod discover;
pub mod render;
pub mod validate;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::device::{Color, DisplayElement, DisplayElements, Priority};
use crate::error::CliError;
use crate::sanitize;

/// A parsed `template.toml`.
///
/// This is `DisplayElements` minus `application_name`, plus `description`.
/// The architecture doc says a template deserializes directly into
/// `DisplayElements`; measured, it cannot — `application_name` is required and
/// has no default, and a template must not carry it, because which app owns
/// the draw comes from `--app`/`BUSY_APP`/the config file. `description` is
/// the other half: `DisplayElements` has no such field and no
/// `deny_unknown_fields`, so the doc's own example key parses and vanishes.
///
/// `elements` is `Vec<DisplayElement>` — busylib's own type — so `animation`,
/// `rectangle`, and `countdown` still come along free.
// `description` is read by `cmd::template::{list, show}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFile {
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub led_notification_color: Option<Color>,
    #[serde(default)]
    pub elements: Vec<DisplayElement>,
}

impl TemplateFile {
    /// Supply the envelope the template deliberately omits.
    pub fn into_payload(self, app: &str) -> Result<DisplayElements, CliError> {
        let app = crate::device::AppName::new(app.to_owned())
            .map_err(|error| CliError::usage(format!("invalid --app: {error}")))?;

        let mut payload =
            DisplayElements::new(app).map_err(|error| CliError::usage(error.to_string()))?;

        if let Some(priority) = self.priority {
            payload = payload.priority(priority);
        }
        if let Some(color) = self.led_notification_color {
            payload = payload.led_notification_color(color);
        }
        for element in self.elements {
            payload = payload.element(element);
        }
        Ok(payload)
    }
}

/// A template on disk, loaded but not yet rendered.
// `dir` is read by `cmd::template::show` (and used to resolve a template's
// relative asset paths, e.g. `template::validate::offline`).
#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub dir: PathBuf,
    pub source: String,
}

impl Template {
    /// Read `<root>/<name>/template.toml`.
    pub fn load(root: &Path, name: &str) -> Result<Self, CliError> {
        discover::validate_name(name)?;
        let dir = root.join(name);
        let path = dir.join("template.toml");

        let source = std::fs::read_to_string(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                let candidates = discover::list(root);
                let hint = match discover::suggest(name, &candidates) {
                    Some(near) => format!(" Did you mean `{near}`?"),
                    None if candidates.is_empty() => {
                        " No templates are installed; run `busy template init`.".to_owned()
                    }
                    None => format!(" Available: {}.", candidates.join(", ")),
                };
                CliError::usage(format!("no template named `{name}`.{hint}"))
            } else {
                // The template exists but reading it failed — permissions, a
                // broken symlink, an I/O error. That is not the user getting
                // the command wrong, so it must not exit like a usage error.
                CliError::runtime(format!("could not read {}: {error}", path.display()))
            }
        })?;

        Ok(Self {
            name: name.to_owned(),
            dir,
            source,
        })
    }

    /// The variables this template references.
    pub fn required_variables(&self) -> Result<Vec<String>, CliError> {
        render::analyse(&self.name, &self.source)
    }

    /// Compare this template's statically-analysed required variables
    /// against what was actually supplied (`bind_variables`'s output), and
    /// fail with a real message naming every missing one.
    ///
    /// Without this, a missing variable only surfaces once `render` hits it
    /// mid-substitution, as minijinja's raw `undefined value (in
    /// <name>:<line>)` — which names neither the variable nor how to supply
    /// it. Forgetting a variable (`message`, above all) is the single most
    /// common mistake a template user makes, so it is worth catching with
    /// the static analysis `required_variables` already runs, before
    /// rendering, rather than leaving strict-undefined as the only report.
    /// `render`'s strict-undefined behaviour stays in place regardless, as
    /// the backstop for anything this comparison does not catch — a
    /// variable used only inside a branch this analysis still sees, but a
    /// caller happens to supply under a name that doesn't match, say.
    ///
    /// `--var <name>=…` is the only way to supply anything but `message`,
    /// which doubles as the positional argument, so "pass it positionally"
    /// is only ever suggested for `message`.
    pub fn check_required_variables(
        &self,
        supplied: &BTreeMap<String, String>,
    ) -> Result<(), CliError> {
        let required = self.required_variables()?;
        let missing: Vec<String> = required
            .into_iter()
            .filter(|key| !supplied.contains_key(key.as_str()))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        // `required_variables` is static analysis over raw variable references;
        // it cannot see that `{{ x | default(...) }}` makes `x` optional (the
        // same over-report `template show` already warns about for a
        // branch-only reference — see its doc comment). Rendering is proof:
        // minijinja's own `default` filter resolves an absent value before
        // strict-undefined ever fires, so if the template renders fine despite
        // the gap, nothing on `missing` was actually required. This is what
        // lets `busy draw ok` work with no arguments even though `message` is
        // technically "referenced".
        if self.render(supplied).is_ok() {
            return Ok(());
        }
        Err(missing_variables_error(&self.name, &missing))
    }

    /// Render and parse. The two are one step because a rendered template that
    /// does not parse is a template error, reported against the template name.
    pub fn render(&self, vars: &BTreeMap<String, String>) -> Result<TemplateFile, CliError> {
        let rendered = render::render(&self.name, &self.source, vars)?;
        toml::from_str::<TemplateFile>(&rendered).map_err(|error| {
            CliError::usage(format!(
                "template `{}` did not produce a valid template file: {error}",
                self.name
            ))
        })
    }
}

/// Build the "template `x` requires variable(s) …" error for
/// `Template::check_required_variables`. `missing` must be non-empty and
/// already sorted (it comes straight from `required_variables`, which
/// sorts).
///
/// `message` gets its own clause ("pass it as the positional argument")
/// alongside `--var message=…`, because it is the one variable with a second
/// way in. Every other missing variable only ever gets the `--var` form —
/// suggesting a positional for anything else would describe a flag that
/// does not exist.
fn missing_variables_error(name: &str, missing: &[String]) -> CliError {
    let var_flags = |names: &[String]| -> String {
        names
            .iter()
            .map(|var| format!("`--var {var}=…`"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let has_message = missing.iter().any(|var| var == "message");
    let others: Vec<String> = missing
        .iter()
        .filter(|var| *var != "message")
        .cloned()
        .collect();

    let noun = if missing.len() == 1 {
        "variable"
    } else {
        "variables"
    };
    let names = missing
        .iter()
        .map(|var| format!("`{var}`"))
        .collect::<Vec<_>>()
        .join(", ");

    let advice = match (has_message, others.is_empty()) {
        (true, true) => "pass it as the positional argument or `--var message=…`".to_owned(),
        (true, false) => format!(
            "pass `message` as the positional argument, and the rest with {}",
            var_flags(&others)
        ),
        (false, _) if missing.len() == 1 => format!("pass it with `--var {}=…`", missing[0]),
        (false, _) => format!("pass them with {}", var_flags(missing)),
    };

    CliError::usage(format!(
        "template `{name}` requires {noun} {names}; {advice}."
    ))
}

/// Collect variable values from the positional argument and repeated `--var`.
///
/// The positional binds to `message`, which is the one variable common enough
/// to deserve a positional. Supplying it both ways is an error rather than a
/// silent precedence rule.
///
/// Every value is sanitized to printable ASCII before it is returned — see
/// `sanitize_values` — so the second element of the tuple is whether any of
/// them needed it, for the caller to fold into a single once-per-invocation
/// warning.
pub fn bind_variables(
    positional: Option<&str>,
    pairs: &[String],
) -> Result<(BTreeMap<String, String>, bool), CliError> {
    let mut vars = BTreeMap::new();

    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(CliError::usage(format!(
                "`--var {pair}` is not in `k=v` form; write `--var {pair}=<value>`."
            )));
        };
        validate_var_key(key)?;
        // An empty value (`k=`) is left alone: `--var note=` is a legitimate
        // way to bind an empty string. A key supplied more than once takes
        // its last value here — the conventional shell-flag behaviour — as
        // opposed to the positional/`message` collision below, which errors
        // instead of picking a winner.
        vars.insert(key.to_owned(), value.to_owned());
    }

    if let Some(message) = positional {
        if vars.contains_key("message") {
            return Err(CliError::usage(
                "the message was supplied both as a positional argument and as \
                 `--var message=…`; use one or the other.",
            ));
        }
        vars.insert("message".to_owned(), message.to_owned());
    }

    let changed = sanitize_values(&mut vars);
    Ok((vars, changed))
}

/// Sanitize every variable value to printable ASCII in place, the same
/// transliteration `busy text` applies to its message (`sanitize::to_ascii`).
/// Returns whether anything changed, so a caller can warn once per
/// invocation instead of once per variable.
///
/// This is not optional, and not just a nicety: `TextElement.text` is
/// busylib's `Text`, whose `Deserialize` impl rejects non-ASCII bytes
/// outright. A raw smart quote in a `--var` value would otherwise survive
/// substitution and then fail deep inside `Template::render`'s
/// `toml::from_str` as an opaque parse error naming the template — not the
/// sanitize-and-warn experience `busy text` gives the exact same character.
/// A template must not be more fragile than a message.
///
/// Every path that renders a template must run values through this before
/// calling `Template::render`. `bind_variables` (above) is the chokepoint
/// for `--var` and the positional message; it is not the only one, though:
/// `template validate`'s placeholder binding does not call
/// `bind_variables` at all (its "values" are one synthetic placeholder per
/// required variable, not user-typed `k=v` pairs), so `cmd::template::validate`
/// calls this function directly on its own placeholder map to stay on the
/// same guarantee.
pub fn sanitize_values(vars: &mut BTreeMap<String, String>) -> bool {
    let mut changed = false;
    for value in vars.values_mut() {
        let sanitized = sanitize::to_ascii(value);
        changed |= sanitized.changed;
        *value = sanitized.text;
    }
    changed
}

/// Reject a `--var` key that no template could ever reference. minijinja
/// variables are identifiers: ASCII letters, digits, or underscore, and not
/// starting with a digit. `9x`, `a-b`, and a key with leading or trailing
/// whitespace would all otherwise parse, bind, and then silently do nothing
/// — the user's typo would vanish instead of erroring. Nothing here is
/// trimmed: whitespace in a key is a mistake, not something to fix on the
/// user's behalf.
fn validate_var_key(key: &str) -> Result<(), CliError> {
    let mut chars = key.chars();
    let starts_ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if starts_ok && rest_ok {
        return Ok(());
    }
    Err(CliError::usage(format!(
        "`{key}` is not a usable variable name: use ASCII letters, digits, or \
         underscore, and do not start with a digit."
    )))
}

#[cfg(test)]
mod tests {
    use super::{CliError, Template, TemplateFile, bind_variables};

    #[test]
    fn a_template_file_becomes_a_payload_with_the_app_name_supplied() {
        // `DisplayElements::application_name` is required and has no default,
        // so a template cannot deserialize into it directly — verified. The
        // owning app comes from --app/BUSY_APP/config, never from the file.
        let file: TemplateFile = toml::from_str(
            r#"
            description = "an example"
            priority = 42
            [[elements]]
            id = "message"
            type = "text"
            text = "hi"
            font = "small"
            "#,
        )
        .expect("should parse");
        assert_eq!(file.description.as_deref(), Some("an example"));

        let payload = file.into_payload("busy").expect("should build");
        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(json.contains("\"application_name\":\"busy\""), "got {json}");
        assert!(json.contains("\"priority\":42"), "got {json}");
    }

    #[test]
    fn element_kinds_this_project_does_not_model_survive_the_round_trip() {
        // The whole reason a template deserializes into busylib's own types:
        // rectangle, countdown, and animation come along free.
        let file: TemplateFile = toml::from_str(
            r#"
            [[elements]]
            id = "bar"
            type = "rectangle"
            width = 40
            height = 4
            "#,
        )
        .expect("should parse");
        let payload = file.into_payload("busy").expect("should build");
        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(json.contains("\"type\":\"rectangle\""), "got {json}");
        assert!(json.contains("\"width\":40"), "got {json}");
    }

    #[test]
    fn an_unknown_key_is_an_error_rather_than_being_discarded() {
        // DisplayElements has no deny_unknown_fields, so `descriptoin = ".."`
        // would parse and vanish. The wrapper adds it back.
        let error = toml::from_str::<TemplateFile>("descriptoin = \"typo\"\nelements = []")
            .expect_err("should reject");
        assert!(error.to_string().contains("descriptoin"), "got {error}");
    }

    #[test]
    fn the_positional_binds_to_message() {
        let (bound, changed) = bind_variables(Some("Build failed"), &[]).expect("should bind");
        assert_eq!(
            bound.get("message").map(String::as_str),
            Some("Build failed")
        );
        assert!(!changed, "plain ASCII should not report a change");
    }

    #[test]
    fn var_pairs_are_parsed_and_may_contain_equals_signs() {
        let (bound, changed) = bind_variables(None, &["code=500".to_owned(), "url=a=b".to_owned()])
            .expect("should bind");
        assert_eq!(bound.get("code").map(String::as_str), Some("500"));
        assert_eq!(bound.get("url").map(String::as_str), Some("a=b"));
        assert!(!changed);
    }

    #[test]
    fn a_var_without_an_equals_sign_is_a_usage_error() {
        let error = bind_variables(None, &["justakey".to_owned()])
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("justakey"), "got {error}");
        assert!(error.contains("k=v"), "should show the form, got {error}");
    }

    #[test]
    fn supplying_the_message_twice_is_an_error() {
        let error = bind_variables(Some("one"), &["message=two".to_owned()])
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("message"), "got {error}");
    }

    #[test]
    fn a_key_that_is_not_a_valid_identifier_is_rejected() {
        // minijinja variables are identifiers; a key that can never match one
        // would silently do nothing instead of erroring, hiding a typo.
        for bad in ["9x=1", "a-b=1", " a=1", "a b=1", "a =1"] {
            let error = bind_variables(None, &[bad.to_owned()])
                .expect_err(&format!("{bad:?} should be rejected"))
                .to_string();
            assert!(error.contains("not a usable variable name"), "got {error}");
        }
    }

    #[test]
    fn an_empty_value_is_accepted() {
        // `--var note=` legitimately binds an empty string.
        let (bound, _) = bind_variables(None, &["note=".to_owned()]).expect("should bind");
        assert_eq!(bound.get("note").map(String::as_str), Some(""));
    }

    #[test]
    fn a_repeated_key_takes_the_last_value() {
        // The conventional shell-flag behaviour: last one wins, no error.
        let (bound, _) =
            bind_variables(None, &["code=1".to_owned(), "code=2".to_owned()]).expect("should bind");
        assert_eq!(bound.get("code").map(String::as_str), Some("2"));
    }

    #[test]
    fn a_variable_value_with_a_smart_quote_is_sanitized_and_reported() {
        // The addition to Task 5's brief: a template must not be more
        // fragile than a message. `busy text` transliterates a smart quote
        // and warns; `--var`/the positional message must get the same
        // treatment before the value ever reaches `Template::render`, or a
        // routine `git log -1 --format=%s` value would hard-fail template
        // rendering with an opaque TOML parse error instead.
        let (bound, changed) = bind_variables(Some("It\u{2019}s done"), &[]).expect("should bind");
        assert_eq!(bound.get("message").map(String::as_str), Some("It's done"));
        assert!(changed, "a transliterated value must report a change");
    }

    #[test]
    fn a_sanitized_variable_value_still_renders_successfully() {
        // Proves the sanitizing actually unblocks the render, not just that
        // the string comes out looking right in isolation: the whole point
        // is that `Template::render` must not see the raw non-ASCII byte.
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(
            dir.join("greet/template.toml"),
            "description = \"{{ message }}\"\nelements = []\n",
        )
        .unwrap();

        let (vars, changed) = bind_variables(Some("It\u{2019}s done"), &[]).expect("should bind");
        assert!(changed);

        let template = Template::load(&dir, "greet").expect("should load");
        let file = template
            .render(&vars)
            .expect("sanitized value should render");
        assert_eq!(file.description.as_deref(), Some("It's done"));
    }

    #[test]
    fn led_notification_color_round_trips_into_the_payload() {
        // Not verified by the brief's author — checked here rather than
        // assumed. `Color` deserializes from a plain string via
        // `String::deserialize` + `Color::parse`, so a top-level
        // `led_notification_color = "#0000ffff"` should parse and survive
        // into the wire payload untouched.
        let file: TemplateFile = toml::from_str(
            r##"
            led_notification_color = "#0000ffff"
            elements = []
            "##,
        )
        .expect("should parse");
        let payload = file.into_payload("busy").expect("should build");
        let json = serde_json::to_string(&payload).expect("should serialize");
        assert!(
            json.contains("\"led_notification_color\":\"#0000FFFF\""),
            "got {json}"
        );
    }

    // `Template::load`/`required_variables`/`render` are part of Task 3's
    // interface (see the brief's "Produces" list) but the brief's own Step 2
    // test list never exercises them directly — only `TemplateFile` and
    // `bind_variables`. Added here so the type actually has coverage rather
    // than relying on later tasks to be the first caller.

    #[test]
    fn load_reads_a_templates_toml_from_disk() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(dir.join("greet/template.toml"), "elements = []").unwrap();

        let template = Template::load(&dir, "greet").expect("should load");
        assert_eq!(template.name, "greet");
        assert_eq!(template.dir, dir.join("greet"));
        assert_eq!(template.source, "elements = []");
    }

    #[test]
    fn loading_a_missing_template_names_it_and_offers_a_hint() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("error")).unwrap();
        std::fs::write(dir.join("error/template.toml"), "elements = []").unwrap();

        let error = Template::load(&dir, "eror").expect_err("should fail");
        assert!(
            matches!(error, CliError::Usage(_)),
            "a missing template is a usage error, got {error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("eror"), "got {message}");
        assert!(message.contains("error"), "should suggest, got {message}");
    }

    #[test]
    #[cfg(unix)]
    fn an_unreadable_template_is_a_runtime_error_not_a_usage_error() {
        // Prerequisite P2 on this branch existed to stop runtime failures
        // (I/O errors, permissions) from being mislabelled as usage errors;
        // an unreadable-but-present file is exactly that case, distinct from
        // `NotFound`, which genuinely is a usage error (the user asked for a
        // template that is not there).
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempdir();
        std::fs::create_dir_all(dir.join("locked")).unwrap();
        let path = dir.join("locked/template.toml");
        std::fs::write(&path, "elements = []").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = Template::load(&dir, "locked");

        // Restore permissions unconditionally so the temp dir cleans up (and
        // so a failed assertion below doesn't leave an unreadable file
        // behind for the next run to trip over).
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let error = result.expect_err("should fail to read");
        assert!(
            matches!(error, CliError::Runtime(_)),
            "an unreadable file is a runtime failure, not a usage error; got {error:?}"
        );
    }

    #[test]
    fn loading_an_invalid_name_is_rejected_before_touching_disk() {
        // Asserting only on `../escape` appearing in the message does not
        // discriminate: a `NotFound` read of `root/../escape/template.toml`
        // would produce "no template named `../escape`...", which contains
        // the same substring. Assert on wording only `validate_name`
        // produces, so deleting the `validate_name` call in `Template::load`
        // makes this test fail rather than pass for the wrong reason —
        // verified by removing that call and re-running this test alone.
        let dir = tempdir();
        let error = Template::load(&dir, "../escape")
            .expect_err("should fail")
            .to_string();
        assert!(
            error.contains("not a usable template name"),
            "should come from validate_name, got {error}"
        );
    }

    #[test]
    fn required_variables_delegates_to_analyse() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(
            dir.join("greet/template.toml"),
            "text = \"{{ message }}\"\n",
        )
        .unwrap();

        let template = Template::load(&dir, "greet").expect("should load");
        let vars = template.required_variables().expect("should analyse");
        assert_eq!(vars, vec!["message".to_owned()]);
    }

    #[test]
    fn a_missing_required_variable_names_the_template_and_the_variable() {
        // Fix round 1: minijinja's own "undefined value (in error:3)" names
        // neither, which is what this check exists to replace.
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("error")).unwrap();
        std::fs::write(
            dir.join("error/template.toml"),
            "text = \"{{ message }}\"\n",
        )
        .unwrap();

        let template = Template::load(&dir, "error").expect("should load");
        let error = template
            .check_required_variables(&std::collections::BTreeMap::new())
            .expect_err("should reject")
            .to_string();
        assert!(
            error.contains("error"),
            "should name the template, got {error}"
        );
        assert!(
            error.contains("message"),
            "should name the variable, got {error}"
        );
        assert!(
            error.contains("positional argument"),
            "message has a positional, so it should be offered, got {error}"
        );
    }

    #[test]
    fn two_missing_required_variables_are_both_named() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("two")).unwrap();
        std::fs::write(
            dir.join("two/template.toml"),
            "text = \"{{ first }} {{ second }}\"\n",
        )
        .unwrap();

        let template = Template::load(&dir, "two").expect("should load");
        let error = template
            .check_required_variables(&std::collections::BTreeMap::new())
            .expect_err("should reject")
            .to_string();
        assert!(error.contains("first"), "got {error}");
        assert!(error.contains("second"), "got {error}");
        assert!(
            !error.contains("positional"),
            "neither variable is `message`, so no positional should be offered, got {error}"
        );
    }

    #[test]
    fn a_supplied_variable_is_not_reported_as_missing() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(
            dir.join("greet/template.toml"),
            "text = \"{{ message }}\"\n",
        )
        .unwrap();

        let template = Template::load(&dir, "greet").expect("should load");
        let mut vars = std::collections::BTreeMap::new();
        vars.insert("message".to_owned(), "hi".to_owned());
        template
            .check_required_variables(&vars)
            .expect("all required variables are supplied");
    }

    #[test]
    fn a_variable_with_a_minijinja_default_is_not_required() {
        // `required_variables` cannot see through `| default(...)` — it is
        // static analysis over raw references, not evaluation — so it lists
        // `message` here even though the shipped `ok` example relies on this
        // exact pattern to work with no arguments at all. `render` succeeding
        // despite the gap is what makes the flag-that-has-a-default distinct
        // from a variable that is genuinely missing.
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(
            dir.join("greet/template.toml"),
            "description = \"{{ message | default('Done') }}\"\nelements = []\n",
        )
        .unwrap();

        let template = Template::load(&dir, "greet").expect("should load");
        assert_eq!(
            template.required_variables().expect("should analyse"),
            vec!["message".to_owned()],
            "static analysis still over-reports message as referenced"
        );
        template
            .check_required_variables(&std::collections::BTreeMap::new())
            .expect("a default filter makes the gap harmless");
    }

    #[test]
    fn render_substitutes_variables_and_parses_the_result() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("greet")).unwrap();
        std::fs::write(
            dir.join("greet/template.toml"),
            "description = \"{{ message }}\"\nelements = []\n",
        )
        .unwrap();

        let template = Template::load(&dir, "greet").expect("should load");
        let mut vars = std::collections::BTreeMap::new();
        vars.insert("message".to_owned(), "hi".to_owned());

        let file = template.render(&vars).expect("should render");
        assert_eq!(file.description.as_deref(), Some("hi"));
    }

    #[test]
    fn render_names_the_template_when_the_rendered_output_does_not_parse() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("broken")).unwrap();
        std::fs::write(dir.join("broken/template.toml"), "not valid toml [[[").unwrap();

        let template = Template::load(&dir, "broken").expect("should load");
        let error = template
            .render(&std::collections::BTreeMap::new())
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("broken"), "got {error}");
    }

    /// A unique temp directory for one test. `std::env::temp_dir()` is shared,
    /// and the suite runs in parallel.
    fn tempdir() -> std::path::PathBuf {
        let unique = format!(
            "busy-template-mod-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
