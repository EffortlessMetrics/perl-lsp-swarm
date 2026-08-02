// Deterministic combined-tree regression fixture for #4556.

use color_eyre::eyre::{Context, Result, eyre};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[derive(Clone, Copy)]
enum CandidateKind {
    Collision,
    Unrelated,
}

fn write_base(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"merge-integration-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        root.join("src/lib.rs"),
        "mod first;\n// EXPORT_SLOT_A\nmod second;\n\n// EXPORT_SLOT_B\n",
    )?;
    fs::write(root.join("src/first.rs"), "")?;
    fs::write(root.join("src/second.rs"), "")?;
    Ok(())
}

fn apply_candidate(root: &Path, kind: CandidateKind, first: bool) -> Result<()> {
    let module = if first { "first" } else { "second" };
    let module_path = root.join(format!("src/{module}.rs"));
    let module_body = match kind {
        CandidateKind::Collision => "pub struct Collision;\n",
        CandidateKind::Unrelated => "pub struct Distinct;\n",
    };
    fs::write(module_path, module_body)?;

    let lib_path = root.join("src/lib.rs");
    let source = fs::read_to_string(&lib_path)?;
    let (marker, replacement) = if first {
        (
            "// EXPORT_SLOT_A",
            match kind {
                CandidateKind::Collision => "pub use first::Collision;",
                CandidateKind::Unrelated => "pub use first::Distinct;",
            },
        )
    } else {
        (
            "// EXPORT_SLOT_B",
            match kind {
                CandidateKind::Collision => "pub use second::Collision;",
                CandidateKind::Unrelated => "pub use second::Distinct;",
            },
        )
    };
    if !source.contains(marker) {
        return Err(eyre!("fixture marker {marker:?} was already consumed"));
    }
    fs::write(&lib_path, source.replacen(marker, replacement, 1))?;
    Ok(())
}

fn rustc_check(root: &Path) -> Result<Output> {
    let target_dir = root.join("target");
    fs::create_dir_all(&target_dir)?;
    Command::new("rustc")
        .args(["--crate-type", "lib", "--edition", "2021", "--emit", "metadata"])
        .arg(root.join("src/lib.rs"))
        .arg("--out-dir")
        .arg(target_dir)
        .output()
        .with_context(|| format!("spawning rustc check for {}", root.display()))
}

fn assert_success(output: &Output, label: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    Err(eyre!("{label} should compile, stderr: {}", String::from_utf8_lossy(&output.stderr)))
}

#[test]
fn individually_green_candidates_fail_only_after_squash_equivalent_combination() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let base_a = temp.path().join("candidate-a");
    let base_b = temp.path().join("candidate-b");
    let combined = temp.path().join("combined");
    for root in [&base_a, &base_b, &combined] {
        write_base(root)?;
    }
    apply_candidate(&base_a, CandidateKind::Collision, true)?;
    apply_candidate(&base_b, CandidateKind::Collision, false)?;
    apply_candidate(&combined, CandidateKind::Collision, true)?;
    apply_candidate(&combined, CandidateKind::Collision, false)?;

    assert_success(&rustc_check(&base_a)?, "candidate A")?;
    assert_success(&rustc_check(&base_b)?, "candidate B")?;
    let combined_output = rustc_check(&combined)?;
    assert!(!combined_output.status.success(), "combined collision fixture must fail");
    let combined_stderr = String::from_utf8_lossy(&combined_output.stderr);
    assert!(
        combined_stderr.contains("E0252")
            || combined_stderr.contains("defined multiple times")
            || combined_stderr.contains("the name `Collision`"),
        "combined failure must identify the duplicate export: {combined_stderr}"
    );
    Ok(())
}

#[test]
fn unrelated_candidates_remain_green_when_combined() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let candidate_a = temp.path().join("candidate-a");
    let candidate_b = temp.path().join("candidate-b");
    let combined = temp.path().join("combined");
    for root in [&candidate_a, &candidate_b, &combined] {
        write_base(root)?;
    }
    apply_candidate(&candidate_a, CandidateKind::Collision, true)?;
    apply_candidate(&candidate_b, CandidateKind::Unrelated, false)?;
    apply_candidate(&combined, CandidateKind::Collision, true)?;
    apply_candidate(&combined, CandidateKind::Unrelated, false)?;

    assert_success(&rustc_check(&candidate_a)?, "unrelated control A")?;
    assert_success(&rustc_check(&candidate_b)?, "unrelated control B")?;
    assert_success(&rustc_check(&combined)?, "unrelated combined control")?;
    Ok(())
}
