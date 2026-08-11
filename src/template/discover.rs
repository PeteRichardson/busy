//! Finding templates on disk.
//!
//! A template is a directory containing `template.toml`. A directory without
//! one is not a template and is skipped silently rather than reported — the
//! root is a user directory, not a manifest.

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// The template root, highest precedence first: `--template-dir`, then
/// `~/.config/busy/templates`.
///
/// `None` means the platform gave us no config directory at all. A root that
/// simply does not exist yet is `Some` — `list` reports it as empty and
/// `template init` creates it.
// Not yet called outside tests — the commands that consume the template root
// land in later tasks of this phase. `cfg_attr(not(test), ...)` keeps the
// expectation accurate under both `cargo test` and `cargo clippy
// --all-targets`; once real callers exist, drop this and let dead-code
// analysis run normally.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn root(flag: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = flag {
        return Some(path.to_path_buf());
    }
    let strategy = etcetera::choose_base_strategy().ok()?;
    use etcetera::BaseStrategy as _;
    Some(strategy.config_dir().join("busy").join("templates"))
}

/// Reject anything that is not a single path component.
///
/// The name is joined onto the root, so `..` or a separator would let a
/// template name reach outside it. Same charset as `AssetName`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn validate_name(name: &str) -> Result<(), CliError> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        return Ok(());
    }
    Err(CliError::usage(format!(
        "`{name}` is not a usable template name: use only letters, digits, dot, \
         underscore, or hyphen."
    )))
}

/// Every template name under `root`, sorted. Never fails: an unreadable root
/// is indistinguishable from an empty one for this purpose.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn list(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().join("template.toml").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// The closest candidate to `name`, when one is close enough to be worth
/// suggesting. Powers did-you-mean on a misresolved draw.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
pub fn suggest(name: &str, candidates: &[String]) -> Option<String> {
    let threshold = 2.max(name.len() / 3);
    candidates
        .iter()
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|(d, _)| *d <= threshold)
        .min_by_key(|(d, _)| *d)
        .map(|(_, candidate)| candidate.clone())
}

/// Levenshtein distance, two-row variant. A `strsim` dependency for one call
/// site is not worth the tree.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "wired up by later tasks in this phase")
)]
fn distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current = vec![0usize; b_chars.len() + 1];

    for (i, a_char) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b_chars.iter().enumerate() {
            let cost = usize::from(a_char != *b_char);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::{list, suggest, validate_name};

    #[test]
    fn a_name_with_a_path_separator_is_rejected() {
        // The name becomes a directory under the root, so `/` or `..` would
        // let a template name escape it. Rejected before any filesystem access.
        for bad in ["../etc", "a/b", "/abs", ".."] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn ordinary_names_are_accepted() {
        for good in ["error", "ok", "build-status", "deploy_v2", "a.b"] {
            assert!(validate_name(good).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn listing_skips_directories_without_a_template_toml() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("error")).unwrap();
        std::fs::write(dir.join("error/template.toml"), "elements = []").unwrap();
        std::fs::create_dir_all(dir.join("not-a-template")).unwrap();
        std::fs::write(dir.join("loose.txt"), "x").unwrap();

        assert_eq!(list(&dir), vec!["error".to_owned()]);
    }

    #[test]
    fn listing_a_missing_root_is_empty_not_an_error() {
        // A missing root means "no templates", exactly as a missing config
        // file means "no config".
        assert!(list(std::path::Path::new("/nonexistent/templates")).is_empty());
    }

    #[test]
    fn the_flag_wins_over_the_default_root() {
        let chosen = super::root(Some(std::path::Path::new("/tmp/elsewhere")));
        assert_eq!(chosen, Some(std::path::PathBuf::from("/tmp/elsewhere")));
    }

    #[test]
    fn suggest_finds_a_near_match_and_ignores_a_distant_one() {
        let names = vec!["error".to_owned(), "ok".to_owned()];
        assert_eq!(suggest("eror", &names), Some("error".to_owned()));
        assert_eq!(suggest("wildly-different", &names), None);
    }

    /// A unique temp directory for one test. `std::env::temp_dir()` is shared,
    /// and the suite runs in parallel.
    fn tempdir() -> std::path::PathBuf {
        let unique = format!(
            "busy-discover-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
