//! Templates: a directory holding a `template.toml` that renders to a payload.

pub mod discover;
pub mod render;
pub mod validate;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::device::{Color, DisplayElement, DisplayElements, Priority};
use crate::error::CliError;

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
// Not yet constructed outside tests — `Template::render` produces one, but
// `Template` itself has no production caller until Task 5 wires `busy
// template ...` commands. `cfg_attr(not(test), ...)` keeps the expectation
// accurate under both `cargo test` and `cargo clippy --all-targets`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "wired up by later tasks in this phase")
    )]
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
// Not yet constructed outside tests — the commands that consume it land in
// Task 5 of this phase. `cfg_attr(not(test), ...)` keeps the expectation
// accurate under both `cargo test` and `cargo clippy --all-targets`; once
// real callers exist, drop this and let dead-code analysis run normally.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub dir: PathBuf,
    pub source: String,
}

impl Template {
    /// Read `<root>/<name>/template.toml`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "wired up by later tasks in this phase")
    )]
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
                CliError::usage(format!("could not read {}: {error}", path.display()))
            }
        })?;

        Ok(Self {
            name: name.to_owned(),
            dir,
            source,
        })
    }

    /// The variables this template references.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "wired up by later tasks in this phase")
    )]
    pub fn required_variables(&self) -> Result<Vec<String>, CliError> {
        render::analyse(&self.name, &self.source)
    }

    /// Render and parse. The two are one step because a rendered template that
    /// does not parse is a template error, reported against the template name.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "wired up by later tasks in this phase")
    )]
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

/// Collect variable values from the positional argument and repeated `--var`.
///
/// The positional binds to `message`, which is the one variable common enough
/// to deserve a positional. Supplying it both ways is an error rather than a
/// silent precedence rule.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn bind_variables(
    positional: Option<&str>,
    pairs: &[String],
) -> Result<BTreeMap<String, String>, CliError> {
    let mut vars = BTreeMap::new();

    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(CliError::usage(format!(
                "`--var {pair}` is not in `k=v` form; write `--var {pair}=<value>`."
            )));
        };
        if key.is_empty() {
            return Err(CliError::usage(format!(
                "`--var {pair}` has an empty variable name."
            )));
        }
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

    Ok(vars)
}

#[cfg(test)]
mod tests {
    use super::{Template, TemplateFile, bind_variables};

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
        let bound = bind_variables(Some("Build failed"), &[]).expect("should bind");
        assert_eq!(
            bound.get("message").map(String::as_str),
            Some("Build failed")
        );
    }

    #[test]
    fn var_pairs_are_parsed_and_may_contain_equals_signs() {
        let bound = bind_variables(None, &["code=500".to_owned(), "url=a=b".to_owned()])
            .expect("should bind");
        assert_eq!(bound.get("code").map(String::as_str), Some("500"));
        assert_eq!(bound.get("url").map(String::as_str), Some("a=b"));
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

        let error = Template::load(&dir, "eror")
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("eror"), "got {error}");
        assert!(error.contains("error"), "should suggest, got {error}");
    }

    #[test]
    fn loading_an_invalid_name_is_rejected_before_touching_disk() {
        let dir = tempdir();
        let error = Template::load(&dir, "../escape")
            .expect_err("should fail")
            .to_string();
        assert!(error.contains("../escape"), "got {error}");
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
