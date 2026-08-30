//! Executable proof for the issue #5432 variant staging adapter.
//!
//! `release_artifact_size` accepts a measurement only when the archive it is
//! given carries the same bytes as the measured directory, and only when the
//! working tree is clean. Both properties are established by
//! `scripts/ci/release_artifact_size_stage.sh`, which otherwise runs only on a
//! macOS runner nobody watches. These tests exercise it directly with real
//! executables so the packaging contract is proven on every ordinary CI run.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, ensure};
use tempfile::TempDir;

const TARGET: &str = "x86_64-apple-darwin";
const VERSION: &str = "9.9.9";
const BINARIES: [&str; 2] = ["perllsp", "perl-dap"];

/// The workspace root: `xtask/..` without an unwrap on an optional parent.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn script() -> PathBuf {
    project_root().join("scripts/ci/release_artifact_size_stage.sh")
}

fn package_name() -> String {
    format!("perllsp-{VERSION}-{TARGET}")
}

/// Build a throwaway repository root whose release directory holds two real,
/// strippable executables. The test binary itself is the most convenient real
/// ELF/Mach-O available on whichever host runs this suite.
fn staged_root() -> Result<TempDir> {
    let root = tempfile::tempdir()?;
    let build_dir = root.path().join("target").join(TARGET).join("release");
    fs::create_dir_all(&build_dir)?;
    let source = std::env::current_exe().context("locating the test executable")?;
    for binary in BINARIES {
        fs::copy(&source, build_dir.join(binary))
            .with_context(|| format!("seeding {binary} from {}", source.display()))?;
    }
    for extra in ["README.md", "LICENSE-APACHE", "LICENSE-MIT"] {
        fs::write(root.path().join(extra), format!("{extra} placeholder\n"))?;
    }
    Ok(root)
}

fn stage(root: &Path, variant: &str) -> Result<Output> {
    Command::new("bash")
        .arg(script())
        .args([variant, TARGET, VERSION])
        .env("RELEASE_ARTIFACT_SIZE_ROOT", root)
        .output()
        .context("running the staging adapter")
}

fn sha256(path: &Path) -> Result<String> {
    let output = Command::new("sha256sum").arg(path).output().context("hashing a staged file")?;
    ensure!(output.status.success(), "sha256sum failed for {}", path.display());
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .context("empty sha256sum output")?
        .to_string())
}

#[test]
fn staging_packages_stripped_binaries_whose_archive_matches_the_directory() -> Result<()> {
    let root = staged_root()?;
    let unstripped =
        fs::metadata(root.path().join("target").join(TARGET).join("release").join(BINARIES[0]))?
            .len();

    let output = stage(root.path(), "baseline")?;
    ensure!(output.status.success(), "staging failed: {}", String::from_utf8_lossy(&output.stderr));

    let package_dir = root.path().join("target/shadow/baseline").join(package_name());
    let archive =
        root.path().join("target/shadow/baseline").join(format!("{}.tar.gz", package_name()));
    ensure!(archive.is_file(), "no release-shaped archive was produced");
    ensure!(
        package_dir.join("SHA256SUMS.txt").is_file(),
        "the package must carry the release checksum manifest"
    );

    // The exact property `measure_archive` gates on: every archive member is
    // byte-identical to the measured directory.
    let extracted = tempfile::tempdir()?;
    let untar = Command::new("tar")
        .args(["xzf".as_ref(), archive.as_os_str(), "-C".as_ref(), extracted.path().as_os_str()])
        .output()?;
    ensure!(untar.status.success(), "the produced archive could not be extracted");

    for binary in BINARIES {
        let staged = package_dir.join(binary);
        ensure!(staged.is_file(), "`{binary}` was not staged");
        ensure!(
            sha256(&staged)? == sha256(&extracted.path().join(package_name()).join(binary))?,
            "`{binary}` differs between the staged directory and the archive"
        );
    }
    // Strictly smaller, not `<=`: `strip` can only shrink or leave a file
    // alone, so `<=` would hold for an implementation that dropped the `strip`
    // call entirely and could not discriminate one.
    ensure!(
        fs::metadata(package_dir.join(BINARIES[0]))?.len() < unstripped,
        "the staged binary was not stripped, so the post-strip claim would be false"
    );

    // A staging directory outside `target/` would dirty the checkout, and the
    // instrument records `subject_complete` only for a clean tree.
    let mut unexpected: Vec<String> = fs::read_dir(root.path())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            !matches!(name.as_str(), "target" | "README.md" | "LICENSE-APACHE" | "LICENSE-MIT")
        })
        .collect();
    unexpected.sort();
    ensure!(unexpected.is_empty(), "staging wrote outside `target/`: {unexpected:?}");

    Ok(())
}

#[test]
fn staging_keeps_each_variant_in_its_own_directory() -> Result<()> {
    let root = staged_root()?;
    for variant in ["baseline", "candidate"] {
        let output = stage(root.path(), variant)?;
        ensure!(
            output.status.success(),
            "staging {variant} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            root.path().join("target/shadow").join(variant).join(package_name()).is_dir(),
            "`{variant}` was not staged into its own directory"
        );
    }
    Ok(())
}

#[test]
fn staging_rejects_an_unknown_variant() -> Result<()> {
    let root = staged_root()?;
    let output = stage(root.path(), "adopted")?;
    ensure!(
        output.status.code() == Some(2),
        "an unknown variant must be a usage error, not a silent third measurement"
    );
    Ok(())
}

#[test]
fn staging_fails_closed_when_a_binary_was_not_built() -> Result<()> {
    let root = staged_root()?;
    fs::remove_file(root.path().join("target").join(TARGET).join("release").join(BINARIES[1]))?;

    let output = stage(root.path(), "baseline")?;
    ensure!(
        !output.status.success(),
        "a half-built variant must not be packaged as a complete measurement subject"
    );
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains(BINARIES[1]),
        "the failure must name the missing binary"
    );
    Ok(())
}

#[test]
fn staging_fails_closed_when_a_binary_cannot_be_stripped() -> Result<()> {
    // Without a working `strip` the script would fail with "command not found"
    // whatever the file contained, and this test would pass for the wrong
    // reason. The contract is that an unstripped binary is never packaged, so
    // establish that `strip` is present before claiming to have exercised it.
    let has_strip =
        Command::new("strip").arg("--version").output().is_ok_and(|output| output.status.success());
    ensure!(has_strip, "this contract needs a working `strip` on the test host");

    let root = staged_root()?;
    // Unlike release.yml, which tolerates a failed strip, the measurement lane
    // must not compare an unstripped binary against a stripped one.
    fs::write(
        root.path().join("target").join(TARGET).join("release").join(BINARIES[0]),
        "not an executable\n",
    )?;

    let output = stage(root.path(), "baseline")?;
    ensure!(
        !output.status.success(),
        "a binary that could not be stripped must fail the staging step"
    );
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to package unstripped"),
        "the failure must be the packaging refusal, not an unrelated shell error"
    );
    Ok(())
}
