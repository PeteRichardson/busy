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
// Not yet called outside tests — Task 5 (`cmd::template::root`) wires this
// up. `cfg_attr(not(test), ...)` keeps the expectation accurate under both
// `cargo test` and `cargo clippy --all-targets`; once that caller exists,
// drop this and let dead-code analysis run normally.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired up by Task 5 of this phase (cmd::template::root)"
    )
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
pub fn suggest(name: &str, candidates: &[String]) -> Option<String> {
    let threshold = (name.len() / 3 + 1).min(2);
    candidates
        .iter()
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|(d, _)| *d <= threshold)
        .min_by_key(|(d, _)| *d)
        .map(|(_, candidate)| candidate.clone())
}

/// Levenshtein distance, two-row variant. A `strsim` dependency for one call
/// site is not worth the tree.
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

    #[test]
    fn the_threshold_does_not_grow_without_bound_for_long_names() {
        // A long name must not accept a match many edits away: the old
        // formula (`2.max(name.len() / 3)`) grew unboundedly with length,
        // so a 30+ character name would accept a distance-10 "match".
        let names = vec!["error".to_owned()];
        assert_eq!(suggest("deployment-completed-successfully", &names), None);
    }

    #[test]
    fn a_short_name_only_matches_at_distance_one() {
        // A 1- or 2-character name is too short for a distance of 2 to mean
        // anything: it would match almost any candidate.
        let names = vec!["ok".to_owned()];
        assert_eq!(suggest("okk", &names), Some("ok".to_owned()));
        assert_eq!(suggest("xyz", &names), None);
    }

    #[test]
    fn the_cap_is_actually_a_cap_not_just_a_low_floor() {
        // The two tests above happen to pass under the old, unbounded
        // formula too (the distances involved are large enough that both
        // formulas agree). This test picks distances that sit inside the
        // old formula's threshold but outside the new cap, so a reversion
        // to `2.max(name.len() / 3)` is actually caught.
        let one_char_query = "a";
        // distance("a", "abc") == 2: old threshold = max(2, 0) = 2 (would
        // match); new threshold = min(2, 0/3 + 1) = 1 (must not match).
        assert_eq!(suggest(one_char_query, &["abc".to_owned()]), None);

        let long_query = "a".repeat(30);
        let mut near_miss = "a".repeat(25);
        near_miss.push_str("bbbbb");
        // distance == 5: old threshold = max(2, 10) = 10 (would match); new
        // threshold = min(2, 10 + 1) = 2 (must not match).
        assert_eq!(suggest(&long_query, &[near_miss]), None);
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
