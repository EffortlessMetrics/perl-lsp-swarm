//! #16732 — `perllsp --check` must not answer `ok` for `my (base) = @_;`.
//!
//! The product path reads `parser.errors()`, not only `parse()`'s `Result`.
//! This fixture drives the built `perllsp` binary so a parser-only green
//! cannot hide a CLI false pass.

use predicates::prelude::PredicateBooleanExt;

mod support;

fn product_command() -> assert_cmd::Command {
    assert_cmd::Command::new(perl_tdd_support::must(support::product_binary_path()))
}

#[test]
fn check_rejects_sigilless_lexical_list_item() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("sigilless.pl");
    std::fs::write(&file, "use strict;\nmy (base) = @_;\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    product_command()
        .arg("--check")
        .arg(file_str)
        .assert()
        .failure()
        .stdout(predicates::str::contains("FAIL"))
        .stdout(predicates::str::contains("constant item"))
        .stdout(predicates::str::contains("ok").not());
    Ok(())
}

#[test]
fn check_accepts_valid_sigil_list_control() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("valid.pl");
    std::fs::write(&file, "use strict;\nmy ($base) = @_;\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    product_command()
        .arg("--check")
        .arg(file_str)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"))
        .stdout(predicates::str::contains("FAIL").not())
        .stdout(predicates::str::contains("constant item").not());
    Ok(())
}
