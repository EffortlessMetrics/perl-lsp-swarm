//! Ephemeral, same-run RIPR+ verification. This is not a reusable analysis cache.
//!
//! A successful producer compares the written bytes with its in-memory result
//! before publishing a checkpoint outside the checkout. Later steps can verify
//! those bytes and unchanged inputs without running the two sensors again.
//! Ordinary `ripr-plus --check` still recomputes both sensors, including when a
//! restored receipt is admitted into this same-run path.
//!
//! The qualified CI window has no dependency installation or cache restoration
//! between producer and consumer. Registry-cache integrity remains the same
//! instrument premise as pinned RIPR; this is not a hermetic cross-run cache.

use color_eyre::eyre::{Context, Result, bail, eyre};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;

pub(super) fn capture(
    repo: &Path,
    root: &str,
    suppressions: &Path,
    receipt: &Path,
    ripr: &str,
    token: &str,
) -> Result<Value> {
    if !cfg!(target_os = "linux") {
        bail!("same-run RIPR+ checkpoints are qualified only for Linux CI");
    }
    if token.is_empty() {
        bail!("same-run RIPR+ verification needs a nonempty run token");
    }
    // Only the root Cargo output directory is excluded from the input walk.
    // Do not let a caller place analyzed inputs, policies or tracked source in
    // that excluded tree. No gitignore rules or language-extension guesses are
    // used: ignored/untracked source, fixtures and configuration are all bound.
    if Path::new(root).starts_with("target")
        || !receipt.starts_with("target")
        || receipt.components().any(|c| !matches!(c, Component::Normal(_)))
        || !super::run_git_bytes(repo, &["ls-files", "-z", "--", "target"])?.is_empty()
    {
        bail!(
            "same-run RIPR+ requires an untracked root target/ output and source outside target/"
        );
    }
    let repo = repo.canonicalize()?;
    let policy = repo.join(suppressions).canonicalize()?;
    if policy.starts_with(repo.join("target")) {
        bail!("same-run RIPR+ policy cannot be inside target/");
    }
    // With a repo-owned config, RIPR cannot inherit an unbound parent config.
    if !repo.join("ripr.toml").is_file() {
        bail!("same-run RIPR+ requires repository-owned ripr.toml");
    }
    let producer = resolve_binary(ripr)?;
    let consumer = env::current_exe()?.canonicalize()?;
    let cargo_home = env::var_os("CARGO_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").filter(|v| !v.is_empty()).map(|v| PathBuf::from(v).join(".cargo"))
        })
        .ok_or_else(|| eyre!("same-run RIPR+ requires explicit CARGO_HOME or HOME"))?;
    if !cargo_home.is_absolute() {
        bail!("same-run RIPR+ requires an absolute Cargo home");
    }
    Ok(json!({
        "schema_version": 1,
        "run_token": token,
        "repository": repo,
        "head": super::current_head(&repo)?,
        "tree": super::run_git_output(&repo, &["rev-parse", "HEAD^{tree}"])?.trim(),
        "index_sha256": digest(&super::run_git_bytes(&repo, &["ls-files", "--stage", "-z"])?),
        "inputs_sha256": input_digest(&repo)?,
        "root": root,
        "receipt": receipt,
        "policy": policy,
        "policy_sha256": file_digest(&policy)?,
        "ripr": producer,
        "ripr_sha256": file_digest(&producer)?,
        "xtask": consumer,
        "xtask_sha256": file_digest(&consumer)?,
        "environment_sha256": environment_digest(env::vars_os()),
        "cargo_configuration_sha256": cargo_configuration_digest(&repo, &cargo_home)?,
        "helpers": [helper_identity("git", &repo)?, helper_identity("cargo", &repo)?, helper_identity("rustc", &repo)?],
    }))
}

// These are the inputs read by the pinned RIPR 9b914379 analysis owners:
// cargo_input_identity.rs, assertion_dependency.rs, quickcheck_generator.rs
// and git.rs. GitHub's per-step environment is deliberately not an input.
fn environment_digest(environment: impl IntoIterator<Item = (OsString, OsString)>) -> String {
    let mut environment = environment
        .into_iter()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            key.starts_with("RIPR_")
                || key.starts_with("CARGO_")
                || key.starts_with("GIT_")
                || matches!(
                    key.as_ref(),
                    "PATH"
                        | "HOME"
                        | "USERPROFILE"
                        | "RUSTUP_HOME"
                        | "RUSTUP_TOOLCHAIN"
                        | "RUSTFLAGS"
                        | "RUSTC"
                        | "RUSTC_WRAPPER"
                        | "RUSTC_WORKSPACE_WRAPPER"
                )
        })
        .collect::<Vec<_>>();
    environment.sort();
    let mut hash = Sha256::new();
    for (key, value) in environment {
        add(&mut hash, key.as_encoded_bytes());
        add(&mut hash, value.as_encoded_bytes());
    }
    hexadecimal(hash.finalize())
}

fn cargo_configuration_digest(root: &Path, home: &Path) -> Result<String> {
    let mut paths = BTreeSet::from([home.join("config"), home.join("config.toml")]);
    for directory in root.ancestors() {
        for name in [
            ".cargo/config",
            ".cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "Cargo.toml",
            "Cargo.lock",
        ] {
            paths.insert(directory.join(name));
        }
    }
    let mut hash = Sha256::new();
    // Config-relative paths retain their directory authority, including a
    // Cargo home reached through a symlink with a different parent.
    for path in [home, home.parent().ok_or_else(|| eyre!("Cargo home needs a parent"))?] {
        add(&mut hash, path.canonicalize()?.as_os_str().as_encoded_bytes());
    }
    let mut total = 0_usize;
    for path in paths {
        add(&mut hash, path.as_os_str().as_encoded_bytes());
        match fs::metadata(&path) {
            Ok(metadata) if !metadata.is_file() => {
                bail!("Cargo input is not a regular file: {}", path.display())
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                add(&mut hash, b"absent");
                continue;
            }
            Err(error) => return Err(error.into()),
        }
        let bytes = read_bounded(&path)?;
        total += bytes.len();
        if total > 16 * 1024 * 1024 {
            bail!("RIPR+ Cargo configuration exceeds 16 MiB");
        }
        add(&mut hash, b"present");
        add(&mut hash, path.canonicalize()?.as_os_str().as_encoded_bytes());
        add(&mut hash, &bytes);
    }
    Ok(hexadecimal(hash.finalize()))
}

fn helper_identity(name: &str, repo: &Path) -> Result<Value> {
    let path = resolve_binary(name)?;
    let sha256 = file_digest(&path)?;
    let mut identity = json!({"name": name, "path": path, "sha256": sha256});
    if matches!(name, "cargo" | "rustc")
        && let Ok(rustup) = resolve_binary("rustup")
        && file_digest(&rustup)? == sha256
    {
        // rustup proxies share bytes with rustup itself. The selected binary
        // behind the proxy must be bound too; looking at the shim is insufficient.
        let output = Command::new(rustup)
            .args(["which", name])
            .current_dir(repo)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .output()?;
        if !output.status.success() {
            bail!("cannot resolve effective {name} behind rustup for same-run RIPR+");
        }
        let selected = resolve_binary(std::str::from_utf8(&output.stdout)?.trim())?;
        identity["selected_sha256"] = json!(file_digest(&selected)?);
        identity["selected"] = json!(selected);
    }
    Ok(identity)
}

pub(super) fn invalidate(path: &Path, repo: &Path) -> Result<()> {
    external_path(path, repo)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("could not invalidate earlier RIPR+ checkpoint"),
    }
}

pub(super) fn external_path(path: &Path, repo: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| eyre!("checkpoint needs an external parent"))?;
    if parent.canonicalize()?.starts_with(repo.canonicalize()?) {
        bail!("RIPR+ checkpoint must stay outside the checkout and its caches");
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            bail!("RIPR+ checkpoint must be a regular file, not a link")
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn publish(
    path: &Path,
    identity: &Value,
    receipt: &Path,
    expected: &[u8],
) -> Result<()> {
    // The digest comes from the computed packet, not from a file merely claiming
    // the right head. A failed/truncated write can never mint a checkpoint.
    if read_bounded(receipt)? != expected {
        bail!("RIPR+ receipt readback differs from the computed packet");
    }
    let packet = json!({"identity": identity, "receipt_sha256": digest(expected)});
    let parent = path.parent().ok_or_else(|| eyre!("checkpoint needs a parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    temporary.write_all(serde_json::to_string(&packet)?.as_bytes())?;
    temporary.flush()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn verify(path: &Path, identity: &Value, receipt: &Path) -> Result<()> {
    let packet: Value = serde_json::from_slice(&read_bounded(path)?)
        .context("missing or invalid same-run RIPR+ checkpoint")?;
    if packet.get("identity") != Some(identity) {
        bail!("RIPR+ checkpoint inputs, tools, policy or run identity changed");
    }
    if packet.get("receipt_sha256").and_then(Value::as_str)
        != Some(digest(&read_bounded(receipt)?).as_str())
    {
        bail!("RIPR+ receipt bytes changed after successful generation or full verification");
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        bail!("RIPR+ verification file exceeds 1 MiB: {}", path.display());
    }
    Ok(bytes)
}

fn input_digest(repo: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let walk = WalkDir::new(repo).follow_links(true).sort_by_file_name().into_iter().filter_entry(
        |entry| {
            entry.depth() != 1 || (entry.file_name() != ".git" && entry.file_name() != "target")
        },
    );
    for (count, entry) in walk.enumerate() {
        if count >= MAX_ENTRIES {
            bail!("RIPR+ input checkpoint exceeds {MAX_ENTRIES} entries");
        }
        let entry = entry.context("cannot enumerate RIPR+ inputs")?;
        let path = entry.path();
        if !path.canonicalize()?.starts_with(repo) {
            bail!("RIPR+ input escapes the checkout: {}", path.display());
        }
        add(&mut hash, path.strip_prefix(repo)?.as_os_str().as_encoded_bytes());
        if entry.path_is_symlink() {
            add(&mut hash, fs::read_link(path)?.as_os_str().as_encoded_bytes());
        }
        if entry.file_type().is_file() {
            bytes = bytes.saturating_add(entry.metadata()?.len());
            if bytes > MAX_INPUT_BYTES {
                bail!("RIPR+ input checkpoint exceeds 2 GiB");
            }
            add(&mut hash, file_digest(path)?.as_bytes());
        } else if !entry.file_type().is_dir() {
            bail!("RIPR+ input is not a regular file or directory: {}", path.display());
        }
    }
    Ok(hexadecimal(hash.finalize()))
}

fn resolve_binary(binary: &str) -> Result<PathBuf> {
    let path = Path::new(binary);
    if path.components().count() > 1 || path.is_absolute() {
        if is_executable(path)? {
            return path.canonicalize().context("cannot resolve RIPR producer");
        }
        bail!("RIPR producer is not executable: {binary}");
    }
    for directory in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join(binary);
        if is_executable(&path)? {
            return Ok(path.canonicalize()?);
        }
    }
    bail!("cannot find RIPR producer {binary} on PATH")
}

fn is_executable(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    Ok(true)
}

fn file_digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(hexadecimal(hash.finalize()))
}

fn add(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn digest(bytes: &[u8]) -> String {
    hexadecimal(Sha256::digest(bytes))
}

fn hexadecimal(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ripr_plus_checkpoint_readback_refuses_wrong_written_bytes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipt = temp.path().join("receipt.json");
        let checkpoint = temp.path().join("checkpoint.json");
        fs::write(&receipt, b"wrong")?;
        let error = publish(&checkpoint, &json!({"head": "same"}), &receipt, b"computed")
            .err()
            .ok_or_else(|| eyre!("incorrect write unexpectedly verified"))?;
        assert!(error.to_string().contains("readback"));
        assert!(!checkpoint.exists());
        Ok(())
    }

    #[test]
    fn ripr_plus_checkpoint_binds_ignored_and_untracked_inputs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        fs::create_dir(root.join("target"))?;
        fs::write(root.join(".gitignore"), "ignored/\n")?;
        fs::create_dir(root.join("ignored"))?;
        fs::write(root.join("ignored/fixture.data"), "one")?;
        let before = input_digest(&root)?;
        fs::write(root.join("ignored/fixture.data"), "two")?;
        assert_ne!(input_digest(&root)?, before, "gitignored bytes are analyzed inputs too");
        fs::write(root.join("ignored/fixture.data"), "one")?;
        fs::write(root.join("new.snap"), "snapshot")?;
        assert_ne!(input_digest(&root)?, before, "new input presence must move the binding");
        fs::remove_file(root.join("new.snap"))?;
        fs::write(root.join("target/result.json"), "output")?;
        assert_eq!(input_digest(&root)?, before, "dedicated build/output bytes are excluded");
        Ok(())
    }

    #[test]
    fn ripr_plus_checkpoint_binds_analysis_environment_and_external_cargo_inputs() -> Result<()> {
        let empty = environment_digest([]);
        for name in [
            "CARGO_BUILD_TARGET",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTFLAGS",
            "RUSTC_WRAPPER",
            "RUSTUP_TOOLCHAIN",
            "PATH",
            "GIT_CONFIG_GLOBAL",
        ] {
            let first = environment_digest([(name.into(), "first".into())]);
            assert_ne!(first, empty, "{name} presence must bind");
            assert_ne!(
                first,
                environment_digest([(name.into(), "second".into())]),
                "{name} value must bind"
            );
        }
        assert_eq!(empty, environment_digest([("GITHUB_STEP_SUMMARY".into(), "new-step".into())]));
        let outside = tempfile::tempdir()?;
        let root = outside.path().join("checkout");
        let home = outside.path().join("cargo-home");
        fs::create_dir(&root)?;
        fs::create_dir(&home)?;
        fs::create_dir(outside.path().join(".cargo"))?;
        let before = cargo_configuration_digest(&root, &home)?;
        for path in [
            outside.path().join(".cargo/config.toml"),
            outside.path().join("Cargo.lock"),
            home.join("config"),
        ] {
            fs::write(&path, "first")?;
            let first = cargo_configuration_digest(&root, &home)?;
            assert_ne!(first, before, "external Cargo input presence must bind");
            fs::write(&path, "second")?;
            assert_ne!(
                cargo_configuration_digest(&root, &home)?,
                first,
                "external Cargo input bytes must bind"
            );
            fs::remove_file(path)?;
            assert_eq!(cargo_configuration_digest(&root, &home)?, before);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn ripr_plus_checkpoint_rejects_external_input_and_cached_checkpoint() -> Result<()> {
        let repo = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        fs::write(outside.path().join("input"), "external")?;
        std::os::unix::fs::symlink(outside.path().join("input"), repo.path().join("input"))?;
        assert!(input_digest(&repo.path().canonicalize()?).is_err());
        fs::create_dir(repo.path().join("target"))?;
        assert!(invalidate(&repo.path().join("target/checkpoint.json"), repo.path()).is_err());
        fs::write(repo.path().join("target/checkpoint.json"), "cached")?;
        let link = outside.path().join("checkpoint.json");
        std::os::unix::fs::symlink(repo.path().join("target/checkpoint.json"), &link)?;
        assert!(external_path(&link, repo.path()).is_err());
        Ok(())
    }
}
