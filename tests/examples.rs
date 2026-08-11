//! Every shipped example must render, parse, and validate.
//!
//! This is what makes "adding an example is just a commit" safe. It iterates
//! the embedded set rather than naming `error` and `ok`, so it keeps covering
//! whatever is added next.

mod common;

use common::busy;

#[test]
fn init_writes_the_examples_and_they_all_validate() {
    let dir = std::env::temp_dir().join("busy-examples-init");
    let _ = std::fs::remove_dir_all(&dir);

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        dir.join("error/template.toml").is_file(),
        "error not written"
    );
    assert!(dir.join("ok/template.toml").is_file(), "ok not written");

    // The guard: whatever init just wrote must pass full offline validation.
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "validate"])
        .assert()
        .success();
}

#[test]
fn init_skips_an_existing_template_without_force() {
    let dir = std::env::temp_dir().join("busy-examples-skip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(dir.join("error/template.toml"), "# mine\nelements = []\n").expect("write");

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .output()
        .expect("should run");
    assert!(output.status.success());

    let kept = std::fs::read_to_string(dir.join("error/template.toml")).expect("read");
    assert!(
        kept.contains("# mine"),
        "an existing template must not be clobbered"
    );
    assert!(
        dir.join("ok/template.toml").is_file(),
        "new examples still arrive"
    );
}

#[test]
fn init_force_overwrites() {
    let dir = std::env::temp_dir().join("busy-examples-force");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("error")).expect("temp dir");
    std::fs::write(dir.join("error/template.toml"), "# mine\nelements = []\n").expect("write");

    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init", "--force"])
        .assert()
        .success();

    let replaced = std::fs::read_to_string(dir.join("error/template.toml")).expect("read");
    assert!(!replaced.contains("# mine"), "--force must overwrite");
}

/// I7: `template validate` binds a synthetic placeholder for every referenced
/// variable, so it cannot tell "this example needs a message" from "this
/// example is broken" — deleting `| default('Done')` from the shipped `ok`
/// template still passed `validate` while breaking `busy draw ok`, which is
/// exactly the Definition-of-Done case (see the ledger's Task 10 entry). This
/// runs every shipped example the way a user actually would: no arguments at
/// all. Each one must either draw (it has no *required* variables, or they
/// all have a minijinja `default`) or fail naming the missing variable — a
/// missing-variable failure is a legitimate example that needs input, an
/// unrelated failure is a broken example, and only this distinguishes them.
#[test]
fn every_shipped_example_draws_or_names_a_missing_variable_with_no_arguments() {
    let dir = std::env::temp_dir().join("busy-examples-noargs");
    let _ = std::fs::remove_dir_all(&dir);

    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .assert()
        .success();

    let mut any_drew_with_no_arguments = false;
    let mut examples_checked = 0;
    for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
        if !entry.path().join("template.toml").is_file() {
            continue;
        }
        let name = entry.file_name().into_string().expect("utf8 template name");
        examples_checked += 1;

        let output = busy()
            .args(["--template-dir"])
            .arg(&dir)
            .args(["--dry-run", "draw", &name])
            .output()
            .expect("should run");

        match output.status.code() {
            Some(0) => any_drew_with_no_arguments = true,
            Some(2) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains("requires variable"),
                    "`{name}` failed with no arguments for a reason other than a missing \
                     required variable, got {stderr}"
                );
            }
            other => panic!(
                "`{name}` with no arguments exited {other:?}, expected 0 (drew) or 2 \
                 (missing variable); stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }

    assert!(
        examples_checked > 0,
        "the init'd directory had no examples to check"
    );
    assert!(
        any_drew_with_no_arguments,
        "at least one shipped example must draw with no arguments at all — this is the \
         Definition-of-Done case (`busy draw ok`) that a placeholder-binding `validate` cannot see"
    );
}

/// The cheaper, more specific pin for the exact regression that happened: the
/// shipped `ok` example must draw with no arguments, and the rendered text
/// must be the `default('Done')` fallback, not an error.
#[test]
fn the_shipped_ok_example_draws_with_no_arguments_and_says_done() {
    let dir = std::env::temp_dir().join("busy-examples-ok-done");
    let _ = std::fs::remove_dir_all(&dir);

    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .assert()
        .success();

    let output = busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["--dry-run", "draw", "ok"])
        .output()
        .expect("should run");
    assert!(
        output.status.success(),
        "`busy draw ok` with no arguments must succeed: stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(r#""text": "Done""#),
        "got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn every_example_declares_a_description() {
    // `template list` prints it; an example without one is a documentation
    // gap, and these examples ARE the documentation.
    let dir = std::env::temp_dir().join("busy-examples-desc");
    let _ = std::fs::remove_dir_all(&dir);
    busy()
        .args(["--template-dir"])
        .arg(&dir)
        .args(["template", "init"])
        .assert()
        .success();

    for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
        let toml_path = entry.path().join("template.toml");
        if !toml_path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&toml_path).expect("read");
        assert!(
            source.contains("description ="),
            "{} has no description",
            entry.path().display()
        );
    }
}
