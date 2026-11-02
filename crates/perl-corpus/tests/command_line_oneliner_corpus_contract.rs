//! Non-executing contract for the command-line one-liner fixture corpus (#13676).

use std::error::Error;
use std::fs;
use std::path::Path;

const MANIFEST_REL: &str = "fixtures/command_line_oneliners/manifest.json";
const COVERAGE_REL: &str = "fixtures/command_line_oneliners/coverage_report.v1.json";

#[test]
fn manifest_and_coverage_files_exist() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/command_line_oneliners");
    assert!(root.join("manifest.json").is_file(), "seed manifest must exist");
    assert!(
        root.join("coverage_report.v1.json").is_file(),
        "coverage report must be checked in; run cargo xtask command-line-oneliner-corpus"
    );
    Ok(())
}

#[test]
fn manifest_declares_hermetic_runner() -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST_REL))?;
    assert!(text.contains("\"runner\": \"none\""), "corpus must remain non-executing by default");
    Ok(())
}

#[test]
fn coverage_report_declares_generation_command() -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(COVERAGE_REL))?;
    assert!(
        text.contains("command-line-oneliner-corpus"),
        "coverage report must name its generator command"
    );
    Ok(())
}
