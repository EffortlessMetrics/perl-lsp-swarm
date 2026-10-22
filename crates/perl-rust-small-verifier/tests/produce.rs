//! Real producer/canonical owner with mocked Cargo product commands, not product proof.
#![cfg(unix)]

use serde_json::Value;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    path: std::ffi::OsString,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
impl Fixture {
    fn new() -> Result<Self> {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let base =
            std::env::temp_dir().join(format!("perl-producer-cli-{}-{nonce}", std::process::id()));
        let root = base.join("repo");
        let bin = base.join("bin");
        fs::create_dir_all(&root)?;
        fs::create_dir(&bin)?;
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            r#"#!/bin/sh
if [ "$1" = --version ]; then echo 'cargo producer-fixture'; exit 0; fi
printf '%s|%s\n' "$*" "${INSTA_UPDATE:-unset}" >> "$PRODUCER_LOG"
if [ "$PRODUCER_MODE" = failure ] && [ "$1" = check ]; then exit 23; fi
if [ "$PRODUCER_MODE" = instrument ] && [ "$1" = fetch ]; then /bin/rm "$0"; fi
for arg in "$@"; do
  if [ "$arg" = --list ]; then printf 'canonical_fixture: test\n'; fi
done
"#,
        )?;
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700))?;
        // No fallback Cargo may survive the missing-instrument control. Retain
        // only real Git/rustc identity tools beside the mocked Cargo executable.
        let original_path = std::env::var_os("PATH").unwrap_or_default();
        for tool in ["git", "rustc"] {
            let executable = std::env::split_paths(&original_path)
                .map(|directory| directory.join(tool))
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| format!("required fixture tool unavailable: {tool}"))?;
            std::os::unix::fs::symlink(executable, bin.join(tool))?;
        }
        let path = bin.into_os_string();
        let fixture = Self { base, root, path };
        fixture.git(&["init", "--quiet"])?;
        fs::write(fixture.root.join("source.txt"), "clean\n")?;
        fixture.git(&["add", "source.txt"])?;
        fixture.git(&[
            "-c",
            "user.name=Producer Test",
            "-c",
            "user.email=producer@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])?;
        Ok(fixture)
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git").args(args).current_dir(&self.root).output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn produce(&self, args: &[&str], mode: &str) -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_perl-rust-small-proof"))
            .args(args)
            .current_dir(&self.root)
            .env("PATH", &self.path)
            .env("PRODUCER_LOG", self.base.join("commands"))
            .env("PRODUCER_MODE", mode)
            .output()?)
    }

    fn receipt(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            self.root.join("target/receipts/rust-small-proof.json"),
        )?)?)
    }
}

#[test]
fn producer_executes_canonical_owner_and_preserves_result_binding() -> Result {
    let fixture = Fixture::new()?;
    let output = fixture.produce(&[], "success")?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let receipt = fixture.receipt()?;
    assert_eq!(receipt["result"], "success");
    assert_eq!(receipt["subject"]["git_sha"], fixture.git(&["rev-parse", "HEAD"])?);
    assert_eq!(receipt["subject"]["worktree_dirty"], false);
    let steps = receipt["steps"].as_array().ok_or("missing steps")?;
    assert_eq!(steps.len(), 10);
    assert_eq!(steps[0]["argv"], serde_json::json!(["cargo", "fetch", "--locked"]));
    assert_eq!(steps[9]["argv"], serde_json::json!(["git", "diff", "--check"]));
    let commands = fs::read_to_string(fixture.base.join("commands"))?;
    assert_eq!(commands.lines().count(), 9);
    assert!(commands.lines().last().ok_or("missing replay")?.ends_with("|no"));
    Ok(())
}

#[test]
fn producer_refuses_every_argument_before_proof() -> Result {
    let fixture = Fixture::new()?;
    for args in [
        vec!["--verify-receipt", "absent.json"],
        vec!["--receipt", "elsewhere"],
        vec!["--"],
        vec!["extra"],
    ] {
        let output = fixture.produce(&args, "success")?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: perl-rust-small-proof"));
        assert!(!fixture.base.join("commands").exists());
        assert!(!fixture.root.join("target").exists());
    }
    Ok(())
}

#[test]
fn producer_retains_failure_receipt_and_stops_remaining_steps() -> Result {
    let fixture = Fixture::new()?;
    let output = fixture.produce(&[], "failure")?;
    assert!(!output.status.success());
    let receipt = fixture.receipt()?;
    assert_eq!(receipt["result"], "product_failure");
    assert_eq!(receipt["steps"][1]["exit_code"], 23);
    assert_eq!(receipt["steps"][2]["outcome"], "not_run");
    assert_eq!(fs::read_to_string(fixture.base.join("commands"))?.lines().count(), 2);
    Ok(())
}

#[test]
fn producer_propagates_missing_instrument_without_success_evidence() -> Result {
    let fixture = Fixture::new()?;
    let output = fixture.produce(&[], "instrument")?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("instrument failure"));
    // Cargo cannot be recaptured after its removal, so no stable receipt is publishable.
    assert!(!fixture.root.join("target/receipts/rust-small-proof.json").exists());
    assert_eq!(fs::read_to_string(fixture.base.join("commands"))?.lines().count(), 1);
    Ok(())
}

#[test]
fn producer_refuses_dirty_source_before_product() -> Result {
    let fixture = Fixture::new()?;
    fs::write(fixture.root.join("source.txt"), "dirty\n")?;
    let output = fixture.produce(&[], "success")?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("clean governed checkout"));
    assert!(!fixture.base.join("commands").exists());
    assert!(!fixture.root.join("target/receipts/rust-small-proof.json").exists());
    Ok(())
}
