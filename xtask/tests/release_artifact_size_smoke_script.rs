//! Executable proof for the issue #5432 per-variant smoke receipt retention.
//!
//! `xtask lsp-ux-smoke` writes its receipt to one fixed path. The baseline and
//! the candidate run in the same job, so there is a real failure mode in which
//! the candidate's comparison silently consumes the baseline's receipt. These
//! tests drive `scripts/ci/release_artifact_size_smoke.sh` with a stub cargo so
//! that retention contract is proven here rather than only on a macOS runner.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, ensure};
use tempfile::TempDir;

const TARGET: &str = "x86_64-apple-darwin";
const VERSION: &str = "9.9.9";

/// The workspace root: `xtask/..` without an unwrap on an optional parent.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn script() -> PathBuf {
    project_root().join("scripts/ci/release_artifact_size_smoke.sh")
}

fn package_dir(root: &Path, variant: &str) -> PathBuf {
    root.join("target/shadow").join(variant).join(format!("perllsp-{VERSION}-{TARGET}"))
}

fn fixed_lsp_receipt(root: &Path) -> PathBuf {
    root.join("target/receipts/ux/lsp-ux-smoke.json")
}

/// A repository root holding staged, executable placeholder binaries. The stub
/// cargo never runs them; only the script's own checks observe them.
fn staged_root(variant: &str) -> Result<TempDir> {
    let root = tempfile::tempdir()?;
    let package = package_dir(root.path(), variant);
    fs::create_dir_all(&package)?;
    for binary in ["perllsp", "perl-dap"] {
        let path = package.join(binary);
        fs::write(&path, "placeholder\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(root)
}

/// Install a stub `cargo` that writes the receipts named by `lsp_receipt` and
/// `dap_receipt`, relative to the staged root. `None` means "this smoke ran but
/// produced no receipt".
fn stub_cargo(root: &Path, lsp_receipt: Option<&str>, dap_receipt: bool) -> Result<PathBuf> {
    let path = root.join("stub-cargo");
    let lsp_line = match lsp_receipt {
        Some(relative) => format!(
            "    mkdir -p \"$(dirname \"{root}/{relative}\")\"\n    \
             printf '{{\"status\":\"pass\",\"binary\":\"stub\"}}\\n' > \"{root}/{relative}\"\n",
            root = root.display(),
        ),
        None => "    :\n".to_string(),
    };
    let dap_line = if dap_receipt {
        "    printf '{\"status\":\"pass\",\"binary\":\"stub\"}\\n' > \"$PERL_DAP_SMOKE_RECEIPT\"\n"
    } else {
        "    :\n"
    };
    fs::write(
        &path,
        format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$*\" >> \"{root}/stub-cargo.argv\"\ncase \"$1\" in\n  run)\n{lsp_line}    ;;\n  \
             test)\n{dap_line}    ;;\nesac\n",
            root = root.display(),
        ),
    )?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

fn smoke(root: &Path, variant: &str, cargo: &Path) -> Result<Output> {
    Command::new("bash")
        .arg(script())
        .args([variant, TARGET, VERSION])
        .env("RELEASE_ARTIFACT_SIZE_ROOT", root)
        .env("CARGO", cargo)
        .output()
        .context("running the smoke adapter")
}

#[test]
fn smoke_retains_each_variant_receipt_under_its_own_directory() -> Result<()> {
    let root = staged_root("candidate")?;
    let cargo = stub_cargo(root.path(), Some("target/receipts/ux/lsp-ux-smoke.json"), true)?;

    let output = smoke(root.path(), "candidate", &cargo)?;
    ensure!(output.status.success(), "smoke failed: {}", String::from_utf8_lossy(&output.stderr));

    let retained = root.path().join("target/shadow/candidate/lsp-smoke.json");
    ensure!(retained.is_file(), "the LSP receipt was not retained for the candidate");
    ensure!(
        root.path().join("target/shadow/candidate/dap-smoke.json").is_file(),
        "the DAP receipt was not retained for the candidate"
    );
    ensure!(
        !fixed_lsp_receipt(root.path()).exists(),
        "the shared receipt path must be emptied so the next variant cannot inherit it"
    );

    // `measure::load_smoke` accepts a receipt only when its `binary` field
    // normalizes to the measured binary. Both smoke tools echo back the path
    // they were handed, so that comparison holds only while the adapter passes
    // an absolute path inside the measured tree. Assert what the adapter
    // actually passed rather than trusting the chain by inspection.
    let argv = fs::read_to_string(root.path().join("stub-cargo.argv"))
        .context("the stub cargo recorded no invocation")?;
    let lsp_binary = argv
        .lines()
        .find_map(|line| line.split("--binary ").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .context("the LSP smoke was invoked without an explicit --binary")?;
    let expected = package_dir(root.path(), "candidate").join("perllsp");
    ensure!(
        Path::new(lsp_binary) == expected,
        "the LSP smoke must be pointed at the exact packaged binary; expected {}, got {lsp_binary}",
        expected.display()
    );
    ensure!(
        argv.lines().any(|line| line.starts_with("test ")),
        "the DAP smoke must run the packaged-binary transport test"
    );
    Ok(())
}

#[test]
fn smoke_never_retains_a_previous_variant_receipt() -> Result<()> {
    let root = staged_root("candidate")?;

    // Simulate the baseline's receipt still sitting at the shared path.
    let stale = fixed_lsp_receipt(root.path());
    fs::create_dir_all(stale.parent().context("receipt directory")?)?;
    fs::write(&stale, "{\"status\":\"pass\",\"binary\":\"baseline\"}\n")?;

    // The candidate's LSP smoke runs but writes nothing.
    let cargo = stub_cargo(root.path(), None, true)?;
    let output = smoke(root.path(), "candidate", &cargo)?;

    ensure!(
        !output.status.success(),
        "a variant whose LSP smoke wrote no receipt must fail, not inherit the previous one"
    );
    ensure!(
        !root.path().join("target/shadow/candidate/lsp-smoke.json").exists(),
        "the stale baseline receipt must never be retained as candidate evidence"
    );
    Ok(())
}

#[test]
fn smoke_fails_closed_when_the_dap_receipt_is_missing() -> Result<()> {
    let root = staged_root("baseline")?;
    let cargo = stub_cargo(root.path(), Some("target/receipts/ux/lsp-ux-smoke.json"), false)?;

    let output = smoke(root.path(), "baseline", &cargo)?;
    ensure!(
        !output.status.success(),
        "a missing DAP receipt must fail the variant rather than reach the comparison"
    );
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("DAP smoke wrote no receipt"),
        "the failure must name the missing DAP receipt"
    );
    Ok(())
}

#[test]
fn smoke_requires_the_variant_to_be_staged_first() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cargo = stub_cargo(root.path(), Some("target/receipts/ux/lsp-ux-smoke.json"), true)?;

    let output = smoke(root.path(), "baseline", &cargo)?;
    ensure!(
        !output.status.success(),
        "smoking an unstaged variant must fail rather than measure nothing"
    );
    ensure!(
        String::from_utf8_lossy(&output.stderr).contains("missing packaged binary"),
        "the failure must name the missing packaged binary"
    );
    Ok(())
}

#[test]
fn smoke_rejects_an_unknown_variant() -> Result<()> {
    let root = staged_root("baseline")?;
    let cargo = stub_cargo(root.path(), Some("target/receipts/ux/lsp-ux-smoke.json"), true)?;

    let output = smoke(root.path(), "adopted", &cargo)?;
    ensure!(
        output.status.code() == Some(2),
        "an unknown variant must be a usage error, not a silent third measurement"
    );
    Ok(())
}
