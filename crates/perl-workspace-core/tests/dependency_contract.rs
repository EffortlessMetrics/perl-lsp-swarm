//! Enforces the PLSP-ADR-0006 / NATIVE_STACK_POLICY dependency contract.
//!
//! The selected graph contains normal and build edges, all targets, and all
//! features of `perl-workspace-core`. Development dependencies are deliberately
//! excluded: this is a production/build boundary, not complete package isolation.
//! Cargo's additive feature resolution still applies; independent unpacked-package
//! consumers remain separate proof. Instrument failure is an unproven contract,
//! never a passing skip.
#![allow(clippy::print_stderr)]
#![deny(clippy::map_err_ignore)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const ROOT_PACKAGE: &str = "perl-workspace-core";
const DIAGNOSTIC_LIMIT: usize = 4096;

/// Exact package identities, never dependency aliases or source-path substrings.
const FORBIDDEN: &[&str] = &[
    "lsp-types",
    "tokio",
    "tower-lsp",
    "effortless-lsp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    "perl-mcp",
    "perl-code-intelligence",
    "perl-workspace",
];

const TREE_COMMAND_ARGS: &[&str] = &[
    "tree",
    "--locked",
    "--color",
    "never",
    "-p",
    ROOT_PACKAGE,
    "--edges",
    "normal,build",
    "--prefix",
    "none",
    "--format",
    "{p}",
    "--target",
    "all",
    "--all-features",
];

fn tree_command(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(directory).args(TREE_COMMAND_ARGS);
    command
}

fn diagnostic(bytes: &[u8]) -> String {
    let end = bytes.len().min(DIAGNOSTIC_LIMIT);
    let text = String::from_utf8_lossy(&bytes[..end]);
    if end == bytes.len() { text.into_owned() } else { format!("{text}\n[truncated]") }
}

fn check_command(command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(|error| {
        format!("dependency contract is unproven: could not run cargo tree: {error}")
    })?;
    check_output(output)
}

fn check_output(output: Output) -> Result<(), String> {
    if !output.status.success() {
        return Err(format!(
            "dependency contract is unproven: cargo tree exited with {}. stderr:\n{}",
            output.status,
            diagnostic(&output.stderr)
        ));
    }
    let tree = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("dependency contract is unproven: non-UTF-8 graph: {error}"))?;
    let names = package_names(tree)?;
    if let Some(forbidden) = names.into_iter().find(|name| FORBIDDEN.contains(name)) {
        return Err(format!(
            "{ROOT_PACKAGE} has forbidden dependency `{forbidden}` in its all-target, \
             all-features normal/build graph; see PLSP-ADR-0006"
        ));
    }
    Ok(())
}

/// Cargo's `{p}` rows are `name vX.Y.Z[ (<source>)][ (*)]`.
/// Reject unrecognizable rows rather than silently dropping them from the proof.
fn package_names(tree: &str) -> Result<Vec<&str>, String> {
    let mut names = Vec::new();
    for (index, line) in tree.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let malformed =
            || format!("dependency contract is unproven: malformed cargo tree row {}", index + 1);
        let (name, remainder) = line.split_once(' ').ok_or_else(malformed)?;
        if name.is_empty()
            || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err(malformed());
        }
        let (version, suffix) = remainder.split_once(' ').unwrap_or((remainder, ""));
        let numeric = version.strip_prefix('v').ok_or_else(malformed)?;
        let release = numeric.split(['-', '+']).next().ok_or_else(malformed)?;
        let components: Vec<_> = release.split('.').collect();
        if components.len() != 3
            || components
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
            || !numeric.bytes().all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
            || (!suffix.is_empty() && !(suffix.starts_with('(') && suffix.ends_with(')')))
        {
            return Err(malformed());
        }
        if names.is_empty() && name != ROOT_PACKAGE {
            return Err(format!(
                "dependency contract is unproven: expected root `{ROOT_PACKAGE}`, got `{name}`"
            ));
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err("dependency contract is unproven: cargo tree returned an empty graph".into());
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

#[test]
fn no_forbidden_dependencies() -> Result<(), String> {
    check_command(&mut tree_command(Path::new(env!("CARGO_MANIFEST_DIR"))))
}

// Synthetic process outputs exercise the same status/graph path as the live check.
// No test mutates the process-global PATH or impersonates Cargo for another test.
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

fn output_fixture(status: i32, stdout: &[u8], stderr: &[u8]) -> Output {
    #[cfg(unix)]
    let status = std::process::ExitStatus::from_raw(status);
    #[cfg(windows)]
    let status = std::process::ExitStatus::from_raw(status.unsigned_abs());
    Output { status, stdout: stdout.to_vec(), stderr: stderr.to_vec() }
}

#[test]
fn unavailable_cargo_cannot_pass() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let result = check_command(&mut Command::new(fixture.0.join("missing-cargo")));
    assert!(result.is_err_and(|error| error.contains("unproven: could not run cargo tree")));
    Ok(())
}

#[test]
fn real_cargo_failure_and_unrelated_success_cannot_pass() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let mut missing_manifest = tree_command(&fixture.0);
    missing_manifest.arg("--manifest-path").arg(fixture.0.join("Cargo.toml"));
    assert!(
        check_command(&mut missing_manifest)
            .is_err_and(|error| error.contains("unproven: cargo tree exited"))
    );
    let result = check_command(Command::new(env!("CARGO")).arg("--version"));
    assert!(result.is_err_and(|error| error.contains("unproven: malformed cargo tree row")));
    Ok(())
}

#[test]
fn unsuccessful_cargo_cannot_pass_even_with_a_valid_graph() {
    let result = check_output(output_fixture(
        1,
        b"perl-workspace-core v0.17.0\n",
        b"failed to resolve a dependency in offline mode",
    ));
    assert!(result.is_err_and(|error| {
        error.contains("unproven: cargo tree exited") && error.contains("offline mode")
    }));
}

#[test]
fn successful_cargo_with_missing_or_invalid_evidence_cannot_pass() {
    for tree in [
        &b""[..],
        b" \n\t\n",
        b"not a dependency graph\n",
        b"serde v1.0.0\nperl-workspace-core v0.17.0\n",
        b"perl-workspace-core v0.17.0\nmalformed\n",
        b"perl-workspace-core invalid-version\n",
        b"perl-workspace-core v0.17.0\nserde v1.0.0 unexpected\n",
        b"perl-workspace-core v0.17.0\n\xff\n",
    ] {
        assert!(
            check_output(output_fixture(0, tree, b""))
                .is_err_and(|error| error.contains("unproven")),
            "invalid graph was accepted: {tree:?}"
        );
    }
}

#[test]
fn package_identity_is_exact_and_valid_graphs_pass() -> Result<(), String> {
    let tree = "perl-workspace-core v0.17.0 (/tmp/perl-workspace/tokio)\n\
                serde v1.0.0\nserde v1.0.0 (*)\n\
                tokio-lookalike v0.1.0-beta.1+build\n";
    assert_eq!(package_names(tree)?, ["perl-workspace-core", "serde", "tokio-lookalike"]);
    check_output(output_fixture(0, tree.as_bytes(), b""))?;
    for forbidden in FORBIDDEN {
        let tree = format!("perl-workspace-core v0.17.0\n{forbidden} v0.1.0\n");
        assert!(check_output(output_fixture(0, tree.as_bytes(), b"")).is_err_and(|error| {
            error.contains(&format!("forbidden dependency `{forbidden}`"))
        }));
    }
    Ok(())
}

#[test]
fn instrument_diagnostics_are_bounded() {
    let stderr = vec![b'x'; DIAGNOSTIC_LIMIT * 2];
    let message = diagnostic(&stderr);
    assert!(message.ends_with("[truncated]"));
    assert!(message.len() < DIAGNOSTIC_LIMIT + 32);
}

/// Disposable, path-only Cargo workspaces: `cargo tree`/lockfile generation do
/// not compile code or need a registry, and cannot mutate the real manifest.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("workspace-dependency-contract-{}-{nonce}-{serial}", std::process::id()));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn workspace(extra: &str) -> Result<Self, String> {
        let fixture = Self::new()?;
        fs::create_dir(fixture.0.join("src")).map_err(|error| error.to_string())?;
        fs::write(fixture.0.join("src/lib.rs"), "").map_err(|error| error.to_string())?;
        fs::write(
            fixture.0.join("Cargo.toml"),
            format!(
                "[package]\nname = '{ROOT_PACKAGE}'\nversion = '0.17.0'\nedition = '2024'\n\
                 [workspace]\n{extra}\n"
            ),
        )
        .map_err(|error| error.to_string())?;
        fs::create_dir(fixture.0.join("runtime")).map_err(|error| error.to_string())?;
        fs::write(
            fixture.0.join("runtime/Cargo.toml"),
            "[package]\nname = 'tokio'\nversion = '0.1.0'\nedition = '2024'\n\
             [lib]\npath = 'lib.rs'\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(fixture.0.join("runtime/lib.rs"), "").map_err(|error| error.to_string())?;
        let output = Command::new(env!("CARGO"))
            .current_dir(&fixture.0)
            .args(["generate-lockfile", "--offline"])
            .output()
            .map_err(|error| format!("could not create fixture lockfile: {error}"))?;
        if !output.status.success() {
            return Err(format!("fixture lockfile failed: {}", diagnostic(&output.stderr)));
        }
        Ok(fixture)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not remove owned dependency fixture {}: {error}", self.0.display());
        }
    }
}

#[test]
fn all_targets_catches_an_optional_renamed_dependency_missing_from_host_graph() -> Result<(), String>
{
    let other_platform = if cfg!(windows) { "unix" } else { "windows" };
    let fixture = Fixture::workspace(&format!(
        "[features]\nwith-runtime = ['dep:renamed-runtime']\n\
         [target.'cfg({other_platform})'.dependencies]\n\
         renamed-runtime = {{ package = 'tokio', path = 'runtime', optional = true }}"
    ))?;
    let mut host = Command::new(env!("CARGO"));
    host.current_dir(&fixture.0)
        .args(TREE_COMMAND_ARGS.iter().copied().filter(|arg| *arg != "--target" && *arg != "all"));
    check_command(&mut host)?;
    let result = check_command(&mut tree_command(&fixture.0));
    assert!(result.is_err_and(|error| error.contains("forbidden dependency `tokio`")));
    Ok(())
}

#[test]
fn build_dependencies_are_checked_and_development_dependencies_are_excluded() -> Result<(), String>
{
    let build = Fixture::workspace("[build-dependencies]\ntokio = { path = 'runtime' }")?;
    assert!(
        check_command(&mut tree_command(&build.0))
            .is_err_and(|error| error.contains("forbidden dependency `tokio`"))
    );
    let development = Fixture::workspace("[dev-dependencies]\ntokio = { path = 'runtime' }")?;
    check_command(&mut tree_command(&development.0))
}
