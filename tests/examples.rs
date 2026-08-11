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
