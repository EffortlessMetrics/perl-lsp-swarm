//! Executable proof for the issue #5432 variant staging adapter.
//!
//! `release_artifact_size` accepts a measurement only when the archive it is
//! given carries the same bytes as the measured directory, and only when the
//! working tree is clean. Both properties are established by
//! `scripts/ci/release_artifact_size_stage.sh`, which otherwise runs only on a
//! macOS runner nobody watches. These tests exercise it directly with real
//! executables. Keep this script-only contract in the CI hygiene crate so its
//! focused target does not build xtask's parser/LSP dependencies and binaries.

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

/// The workspace root, independent of the test process's working directory.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script() -> PathBuf {
    project_root().join("scripts/ci/release_artifact_size_stage.sh")
}

/// MSYS bash strips backslashes from absolute Windows paths passed as argv;
/// forward-slash the script path before handing it to bash on Windows
/// (#15435 / #15423 family C8).
fn script_arg_for_bash() -> String {
    script().to_string_lossy().replace('\\', "/")
}

fn package_name() -> String {
    format!("perllsp-{VERSION}-{TARGET}")
}

/// Own a tiny native executable, including compiler sidecars, until this test ends.
/// Use the existing Rust toolchain rather than requiring a host-specific C driver.
struct ExecutableFixture {
    root: TempDir,
    binary: PathBuf,
}

impl ExecutableFixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("fixture.rs");
        let binary = root.path().join(format!("fixture{}", std::env::consts::EXE_SUFFIX));
        fs::write(&source, "fn main() {}\n")?;
        let mut compiler = Command::new("rustc");
        compiler
            .args(["--edition", "2024", "-C", "opt-level=z", "-C", "panic=abort"])
            // Retain debug/symbol data so removing strip still fails the strict size check.
            .args(["-C", "debuginfo=1", "-C", "strip=none"]);
        // Match .cargo/config.toml's lossless native ELF debug compression.
        // A directly invoked rustc does not inherit Cargo's target rustflags.
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        compiler.args(["-C", "link-arg=-Wl,--compress-debug-sections=zlib"]);
        let output = compiler
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .context("compiling the native executable fixture")?;
        ensure!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(Self { root, binary })
    }
}

/// Reuse the owned fixture for independent copies: strip must never mutate its source.
fn staged_root(fixture: &ExecutableFixture) -> Result<TempDir> {
    let root = tempfile::tempdir()?;
    let build_dir = root.path().join("target").join(TARGET).join("release");
    fs::create_dir_all(&build_dir)?;
    for binary in BINARIES {
        fs::copy(&fixture.binary, build_dir.join(binary))
            .with_context(|| format!("seeding {binary} from {}", fixture.binary.display()))?;
    }
    for extra in ["README.md", "LICENSE-APACHE", "LICENSE-MIT"] {
        fs::write(root.path().join(extra), format!("{extra} placeholder\n"))?;
    }
    Ok(root)
}

fn stage(root: &Path, variant: &str) -> Result<Output> {
    Command::new("bash")
        .arg(script_arg_for_bash())
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
    let fixture = ExecutableFixture::new()?;
    let root = staged_root(&fixture)?;
    // Both inputs are copies of this seed. Capture the oracle before staging.
    let unstripped = fs::metadata(&fixture.binary)?.len();
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
        // Strictly smaller: <= would also pass if staging omitted strip.
        ensure!(
            fs::metadata(&staged)?.len() < unstripped,
            "`{binary}` was not stripped, so the post-strip claim would be false"
        );
    }

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
    let fixture = ExecutableFixture::new()?;
    let root = staged_root(&fixture)?;
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
    // Variant validation precedes binary inspection, so an empty isolated root is sufficient.
    let root = tempfile::tempdir()?;
    let output = stage(root.path(), "adopted")?;
    ensure!(
        output.status.code() == Some(2),
        "an unknown variant must be a usage error, not a silent third measurement"
    );
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("unknown variant adopted"),
        "the usage error must name the unknown variant"
    );
    ensure!(
        fs::read_dir(root.path())?.next().is_none(),
        "an unknown variant must not create staging files"
    );
    Ok(())
}

#[test]
fn staging_fails_closed_when_a_binary_was_not_built() -> Result<()> {
    let fixture = ExecutableFixture::new()?;
    let root = staged_root(&fixture)?;
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

    let fixture = ExecutableFixture::new()?;
    let root = staged_root(&fixture)?;
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

#[test]
#[allow(clippy::print_stderr)]
fn staging_fixture_reuse_and_cleanup_preserve_the_source() -> Result<()> {
    let fixture = ExecutableFixture::new()?;
    let source_hash = sha256(&fixture.binary)?;
    let source_bytes = fs::metadata(&fixture.binary)?.len();
    let test_bytes = fs::metadata(std::env::current_exe()?)?.len();
    eprintln!(
        "fixture seed measurement: test_executable_bytes={test_bytes} \
         native_fixture_bytes={source_bytes} copies_per_root=2"
    );
    ensure!(
        source_bytes < test_bytes,
        "native fixture ({source_bytes} bytes) must be smaller than the test executable ({test_bytes} bytes)"
    );
    for _ in 0..3 {
        let root = staged_root(&fixture)?;
        for binary in BINARIES {
            let seeded = root.path().join("target").join(TARGET).join("release").join(binary);
            ensure!(fs::metadata(&seeded)?.len() == source_bytes, "fixture copy changed size");
            ensure!(sha256(&seeded)? == source_hash, "fixture copy changed bytes");
            // An independently writable copy must not alias the seed or its sibling.
            fs::write(&seeded, "copy isolation probe\n")?;
            ensure!(sha256(&fixture.binary)? == source_hash, "a copy aliases its source fixture");
            for sibling in BINARIES.into_iter().filter(|sibling| *sibling != binary) {
                let sibling_path =
                    root.path().join("target").join(TARGET).join("release").join(sibling);
                ensure!(sha256(&sibling_path)? == source_hash, "fixture copies alias each other");
            }
            fs::copy(&fixture.binary, &seeded)?;
        }
        let output = stage(root.path(), "baseline")?;
        ensure!(
            output.status.success(),
            "reused fixture failed staging: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let root_path = root.path().to_path_buf();
        root.close()?;
        ensure!(!root_path.exists(), "the staging fixture was not cleaned up");
        ensure!(sha256(&fixture.binary)? == source_hash, "staging mutated its source fixture");
        ensure!(
            fs::read_to_string(fixture.root.path().join("fixture.rs"))? == "fn main() {}\n",
            "staging cleanup removed or changed the fixture source"
        );
    }
    let fixture_path = fixture.root.path().to_path_buf();
    fixture.root.close()?;
    ensure!(!fixture_path.exists(), "the executable fixture was not cleaned up");
    Ok(())
}
