//! Offline template checks.
//!
//! Nothing here touches the device. Bounds and overflow checking is not
//! reimplemented here — once rendered, a template *is* a `DisplayElements`,
//! so `crate::validate::bounds_warnings` applies unchanged, and that reuse is
//! the payoff for deserializing into busylib's own types — but the call
//! itself does not live in `offline()`. Every draw path (`cmd::draw::run`)
//! already calls `crate::validate::bounds_warnings` on the final payload
//! exactly once, template or not; `offline()` used to call it a second time
//! for the template path specifically, so a template draw reported every
//! bounds warning twice. `cmd::template::validate` — the caller that
//! actually needed the reuse, since it never goes through `cmd::draw::run` —
//! calls `crate::validate::bounds_warnings` itself instead. `offline()`
//! reports only what is genuinely template-specific: duplicate ids and
//! missing local assets.

use std::collections::HashMap;
use std::path::Path;

use crate::device::{DisplayElements, ElementKind, ImageSource};

/// What validation found. Errors block a draw; warnings do not.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The app-asset paths a payload references. Stock paths are excluded: they
/// are device built-ins with no local file and nothing to upload.
//
// No dead_code annotation: its only production caller is `offline`, below,
// which already carries one. Verified by removing this annotation alone and
// rebuilding — clean, no warning — because rustc's reachability graph
// treats an `expect(dead_code)`-suppressed item as live, which keeps
// everything it calls live too. Adding one here anyway would be inert
// (confirmed: nothing about #[expect(dead_code)] here would be unfulfilled
// were it added back — it's simply unnecessary either way), the same
// redundant-annotation shape this phase's discover.rs cleanup removed.
pub fn referenced_assets(payload: &DisplayElements) -> Vec<String> {
    payload
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementKind::Image(image) => match &image.source {
                ImageSource::Asset { path } => Some(path.to_string()),
                ImageSource::Stock { .. } => None,
            },
            _ => None,
        })
        .collect()
}

/// Check a rendered template against everything knowable without the device
/// that is specific to a template — duplicate ids and missing local assets.
/// Bounds/overflow warnings are deliberately not produced here; see the
/// module doc comment for why the call moved to `cmd::template::validate`.
///
/// Duplicate ids are reported once per id, naming how many elements share it
/// — not once per repetition. The brief's reference implementation pushes an
/// error every time an id repeats, so three elements sharing one id produce
/// two near-identical messages; that's noise, not three separate facts the
/// user needs. One message naming the id and the count says everything the
/// repeated messages said, without making the user count them itself.
///
/// No text-sanitization pass runs here, unlike `busy text`. It would be
/// unreachable: `TextElement.text` is busylib's `Text`, and `Text::deserialize`
/// rejects non-ASCII before a `TemplateFile` — let alone a `DisplayElements`
/// payload — can exist. A template with a smart quote fails to parse (a
/// `CliError::usage` from `Template::render`) long before `offline` would see
/// it; `sanitize::to_ascii` applied to an already-validated `Text` can never
/// report a change. Confirmed: no public busylib API can construct a `Text`
/// holding a non-ASCII byte, so there is no fixture that could ever exercise
/// a sanitize-and-warn branch here. (Task 5 puts the sanitizing pass where it
/// belongs instead: upstream of `Template::render`, in `bind_variables` and
/// `cmd::template::validate`'s placeholder binding — see
/// `template::sanitize_values`.)
pub fn offline(payload: &DisplayElements, dir: &Path) -> Report {
    let mut report = Report::default();

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for element in &payload.elements {
        *counts.entry(element.id.as_str()).or_insert(0) += 1;
    }
    let mut reported_ids: Vec<&str> = Vec::new();
    for element in &payload.elements {
        let id = element.id.as_str();
        let count = counts[id];
        if count > 1 && !reported_ids.contains(&id) {
            reported_ids.push(id);
            report.errors.push(format!(
                "duplicate element id `{id}`: {count} elements share it. Ids are the handle \
                 for `--keep` updates, so a template's own elements would overwrite each other."
            ));
        }
    }

    for asset in referenced_assets(payload) {
        if !dir.join(&asset).is_file() {
            report.errors.push(format!(
                "references `{asset}`, which is not in {}.",
                dir.display()
            ));
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::{Report, offline, referenced_assets};
    use crate::template::TemplateFile;
    use std::path::Path;

    fn payload(toml_src: &str) -> crate::device::DisplayElements {
        toml::from_str::<TemplateFile>(toml_src)
            .expect("test fixture should parse")
            .into_payload("busy")
            .expect("test fixture should build")
    }

    #[test]
    fn report_is_ok_iff_it_has_no_errors() {
        // Not exercised by any `offline` fixture above (none of them produce
        // both zero and nonzero errors from the same call), so covered
        // directly. `Report::is_ok` is otherwise unused until Task 5 wires up
        // `template validate`'s exit code.
        assert!(Report::default().is_ok());
        let with_a_warning_only = Report {
            errors: Vec::new(),
            warnings: vec!["fyi".to_owned()],
        };
        assert!(with_a_warning_only.is_ok(), "warnings do not block a draw");
        let with_an_error = Report {
            errors: vec!["bad".to_owned()],
            warnings: Vec::new(),
        };
        assert!(!with_an_error.is_ok());
    }

    #[test]
    fn duplicate_element_ids_are_an_error() {
        // Ids are the handle for --keep updates, so duplicates make a
        // template's own elements overwrite each other.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "a"
                type = "text"
                text = "one"
                font = "small"
                [[elements]]
                id = "a"
                type = "text"
                text = "two"
                font = "small"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert_eq!(report.errors.len(), 1, "got {:?}", report.errors);
        assert!(report.errors[0].contains("a"), "should name the id");
    }

    #[test]
    fn a_referenced_local_file_that_is_missing_is_an_error() {
        let dir = std::env::temp_dir().join(format!("busy-tv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "icon"
                type = "image"
                path = "stop.png"
                "#,
            ),
            &dir,
        );
        assert_eq!(report.errors.len(), 1, "got {:?}", report.errors);
        assert!(
            report.errors[0].contains("stop.png"),
            "should name the file"
        );
    }

    #[test]
    fn a_stock_path_is_never_treated_as_a_local_file() {
        // `shared/…` is a device built-in; there is no local file to find.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "icon"
                type = "image"
                stock_path = "shared/checkmark_front_8x8.image"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert!(report.errors.is_empty(), "got {:?}", report.errors);
        assert!(
            referenced_assets(&payload(
                r#"
            [[elements]]
            id = "icon"
            type = "image"
            stock_path = "shared/checkmark_front_8x8.image"
            "#
            ))
            .is_empty()
        );
    }

    // The brief's `non_ascii_text_warns_rather_than_failing` is not
    // transcribed: its own fixture cannot be built. `TextElement.text` is
    // busylib's `Text`, whose `Deserialize` rejects non-ASCII bytes — verified
    // by running exactly this fixture through `payload()`, which panicked at
    // the `toml::from_str` step with "invalid display text `don't`: expected
    // one or more printable ASCII characters", never reaching `into_payload`
    // or `offline`. Unlike `busy text`, which sanitizes a raw `&str` from
    // argv before constructing a `Text`, a template's text is deserialized
    // straight into `Text`, so a non-ASCII character is a hard parse failure
    // (a `CliError::usage` from `Template::render`), not a warning. See the
    // comment on `offline` above.

    #[test]
    fn offline_does_not_produce_bounds_warnings_itself() {
        // `offline` used to call `crate::validate::bounds_warnings` directly
        // — "come free from the existing validator" — which was correct in
        // principle (once rendered, a template IS a DisplayElements, so the
        // payload validator applies unchanged) but wrong in practice: every
        // draw path (`cmd::draw::run`) *also* calls `bounds_warnings` on the
        // final payload, so a template draw reported every bounds warning
        // twice. The call now lives only at `cmd::template::validate`'s call
        // site — the caller that doesn't go through `cmd::draw::run` and so
        // still needs it — pinned by
        // `tests/template.rs::validate_still_reports_an_out_of_bounds_element`
        // and `tests/replace.rs`/`tests/overrides.rs`'s draw-path bounds
        // tests, which pin the count stays at one. This test pins the other
        // half: `offline` alone, with an
        // out-of-bounds element, must NOT report it.
        let report = offline(
            &payload(
                r#"
                [[elements]]
                id = "m"
                type = "text"
                text = "hi"
                font = "small"
                x = 900
                y = 0
                align = "top_left"
                "#,
            ),
            Path::new("/nonexistent"),
        );
        assert!(
            report.warnings.is_empty(),
            "offline() must not produce bounds warnings itself any more, got {:?}",
            report.warnings
        );
    }

    #[test]
    fn referenced_assets_lists_only_local_image_paths() {
        let found = referenced_assets(&payload(
            r#"
            [[elements]]
            id = "a"
            type = "image"
            path = "stop.png"
            [[elements]]
            id = "b"
            type = "text"
            text = "hi"
            font = "small"
            "#,
        ));
        assert_eq!(found, vec!["stop.png".to_owned()]);
    }
}
