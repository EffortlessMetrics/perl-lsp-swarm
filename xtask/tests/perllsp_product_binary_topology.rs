use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below repository root")
}

#[test]
fn perl_lsp_rs_is_library_only_and_perllsp_is_the_product_bin() -> Result<()> {
    let root = repository_root()?;
    ensure!(
        !root.join("crates/perl-lsp-rs/src/main.rs").exists(),
        "perl-lsp-rs must not retain src/main.rs"
    );

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .context("running cargo metadata")?;
    ensure!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let packages = metadata["packages"].as_array().context("cargo metadata packages missing")?;

    let implementation = packages
        .iter()
        .find(|package| package["name"] == "perl-lsp-rs")
        .context("perl-lsp-rs package missing")?;
    let implementation_targets =
        implementation["targets"].as_array().context("perl-lsp-rs targets missing")?;
    ensure!(
        implementation_targets.iter().all(|target| {
            target["kind"].as_array().is_none_or(|kinds| !kinds.iter().any(|kind| kind == "bin"))
        }),
        "perl-lsp-rs unexpectedly publishes a binary target"
    );

    let product = packages
        .iter()
        .find(|package| package["name"] == "perllsp")
        .context("perllsp package missing")?;
    let product_targets = product["targets"].as_array().context("perllsp targets missing")?;
    ensure!(
        product_targets.iter().any(|target| {
            target["name"] == "perllsp"
                && target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        }),
        "perllsp must retain the canonical product binary"
    );

    Ok(())
}

#[test]
fn product_launch_consumers_use_perllsp() -> Result<()> {
    let root = repository_root()?;
    let consumers = [
        "justfile",
        "flake.nix",
        ".github/workflows/ci.yml",
        ".github/workflows/ci-nightly.yml",
        "scripts/gate-local.sh",
        "scripts/real-workspace-baseline.sh",
    ];
    for relative in consumers {
        let source = fs::read_to_string(root.join(relative))
            .with_context(|| format!("reading product consumer {relative}"))?;
        ensure!(
            !source.contains("--bin perl-lsp")
                && !source.contains("target/debug/perl-lsp")
                && !source.contains("target/release/perl-lsp")
                && !source.contains("mainProgram = \"perl-lsp\""),
            "{relative} still launches the retired perl-lsp executable"
        );
    }
    Ok(())
}
