//! Compile-boundary proof for the neutral JSON-RPC wire model (#7611).
//!
//! The token-level dependency guard remains useful for broad, cheap scanning,
//! but it cannot prove that an allowed local import does not re-export a Perl
//! taxonomy. This test lets Rust compile the real `jsonrpc.rs` source in a
//! standalone crate whose complete dependency set is `serde` and `serde_json`.
//! The probe declares its own empty workspace, resolves an isolated lockfile,
//! and accepts only checksummed crates.io-registry packages. Its two direct
//! packages are pinned to independently reviewed versions and checksums, so a
//! workspace patch, path dependency, Git source, or unchecksummed replacement
//! cannot become neutral proof merely because the root workspace builds it.
//! A second compile deliberately routes `ErrorClass` through a local
//! `crate::protocol` re-export; that candidate must fail because the probe does
//! not admit `perl-parser-core`.

use std::{
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const JSONRPC_MODEL_PATH: &str = "crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs";
const PROBE_PACKAGE: &str = "lsp-jsonrpc-boundary-probe";
const CRATES_IO_REGISTRY_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const PROBE_MANIFEST: &str = r#"[package]
name = "lsp-jsonrpc-boundary-probe"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
"#;

#[derive(Clone, Copy)]
struct ReviewedRegistryPackage {
    name: &'static str,
    version: &'static str,
    checksum: &'static str,
}

const REVIEWED_DIRECT_PACKAGES: &[ReviewedRegistryPackage] = &[
    ReviewedRegistryPackage {
        name: "serde",
        version: "1.0.229",
        checksum: "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba",
    },
    ReviewedRegistryPackage {
        name: "serde_json",
        version: "1.0.151",
        checksum: "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14",
    },
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live beneath the repository root")
        .to_path_buf()
}

fn rust_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_probe(probe_root: &Path, source: &str) -> io::Result<()> {
    fs::create_dir_all(probe_root.join("src"))?;
    fs::write(probe_root.join("Cargo.toml"), PROBE_MANIFEST)?;
    fs::write(probe_root.join("src/lib.rs"), source)?;
    Ok(())
}

fn cargo_command(probe_root: &Path) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command.current_dir(probe_root);
    command
}

fn generate_probe_lock(probe_root: &Path) -> io::Result<Output> {
    cargo_command(probe_root)
        .arg("generate-lockfile")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(probe_root.join("Cargo.toml"))
        .output()
}

fn check_probe(probe_root: &Path) -> io::Result<Output> {
    cargo_command(probe_root)
        .arg("check")
        .arg("--quiet")
        .arg("--locked")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(probe_root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(probe_root.join("target"))
        .output()
}

fn output_text(output: &Output) -> String {
    fn bounded_stream(label: &str, bytes: &[u8]) -> String {
        let first_non_empty = String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("<empty>")
            .to_owned();
        let bounded: String = first_non_empty.chars().take(200).collect();
        let suffix = if first_non_empty.chars().count() > 200 { "..." } else { "" };
        format!("{label}: {bounded}{suffix}")
    }

    format!(
        "status: {}\n{}\n{}",
        output.status,
        bounded_stream("stdout diagnostic", &output.stdout),
        bounded_stream("stderr diagnostic", &output.stderr)
    )
}

fn validate_probe_lock(lock: &str) -> Result<(), String> {
    let document: toml::Value =
        toml::from_str(lock).map_err(|error| format!("parse probe Cargo.lock: {error}"))?;
    let lock_version = document
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| "probe Cargo.lock has no integer version".to_string())?;
    if lock_version != 4 {
        return Err(format!("probe Cargo.lock version must be 4, got {lock_version}"));
    }

    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "probe Cargo.lock has no package array".to_string())?;
    let mut root_count = 0usize;
    let mut reviewed_counts = vec![0usize; REVIEWED_DIRECT_PACKAGES.len()];

    for package_value in packages {
        let package = package_value
            .as_table()
            .ok_or_else(|| "probe Cargo.lock package is not a table".to_string())?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "probe Cargo.lock package has no name".to_string())?;
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("probe Cargo.lock package {name} has no version"))?;

        if name == PROBE_PACKAGE {
            root_count += 1;
            if package.contains_key("source") || package.contains_key("checksum") {
                return Err("probe root package unexpectedly has source/checksum authority".into());
            }
            let mut direct: Vec<&str> = package
                .get("dependencies")
                .and_then(toml::Value::as_array)
                .map(|deps| deps.iter().filter_map(toml::Value::as_str).collect())
                .unwrap_or_default();
            direct.sort_unstable();
            let mut reviewed: Vec<&str> =
                REVIEWED_DIRECT_PACKAGES.iter().map(|package| package.name).collect();
            reviewed.sort_unstable();
            if direct != reviewed {
                return Err(format!(
                    "probe root direct dependencies {direct:?} must be exactly the reviewed set {reviewed:?}"
                ));
            }
            continue;
        }

        let source = package
            .get("source")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                format!(
                    "probe dependency {name} {version} has no registry source; path and unbound sources are forbidden"
                )
            })?;
        if source != CRATES_IO_REGISTRY_SOURCE {
            return Err(format!(
                "probe dependency {name} {version} uses unreviewed source {source}"
            ));
        }

        let checksum = package
            .get("checksum")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("probe dependency {name} {version} has no checksum"))?;
        if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!(
                "probe dependency {name} {version} has invalid SHA-256 checksum {checksum}"
            ));
        }

        for (index, reviewed) in REVIEWED_DIRECT_PACKAGES.iter().enumerate() {
            if name != reviewed.name {
                continue;
            }
            if version != reviewed.version {
                return Err(format!(
                    "reviewed dependency {name} resolved to {version}, expected {}",
                    reviewed.version
                ));
            }
            if checksum != reviewed.checksum {
                return Err(format!(
                    "reviewed dependency {name} {version} checksum changed from {} to {checksum}",
                    reviewed.checksum
                ));
            }
            reviewed_counts[index] += 1;
        }
    }

    if root_count != 1 {
        return Err(format!(
            "probe Cargo.lock must contain exactly one root package, found {root_count}"
        ));
    }
    for (reviewed, count) in REVIEWED_DIRECT_PACKAGES.iter().zip(reviewed_counts) {
        if count != 1 {
            return Err(format!(
                "reviewed dependency {} {} must appear exactly once, found {count}",
                reviewed.name, reviewed.version
            ));
        }
    }

    Ok(())
}

#[test]
fn probe_lock_rejects_path_git_and_unreviewed_registry_sources() {
    let valid = format!(
        r#"version = 4

[[package]]
name = "{PROBE_PACKAGE}"
version = "0.0.0"
dependencies = [
 "serde",
 "serde_json",
]

[[package]]
name = "serde"
version = "1.0.229"
source = "{CRATES_IO_REGISTRY_SOURCE}"
checksum = "{}"

[[package]]
name = "serde_json"
version = "1.0.151"
source = "{CRATES_IO_REGISTRY_SOURCE}"
checksum = "{}"
"#,
        REVIEWED_DIRECT_PACKAGES[0].checksum, REVIEWED_DIRECT_PACKAGES[1].checksum
    );
    assert!(validate_probe_lock(&valid).is_ok(), "valid reviewed registry lock must pass");

    let path_source = valid.replace(
        &format!(
            "source = \"{CRATES_IO_REGISTRY_SOURCE}\"\nchecksum = \"{}\"\n",
            REVIEWED_DIRECT_PACKAGES[0].checksum
        ),
        "",
    );
    assert!(
        validate_probe_lock(&path_source).is_err(),
        "a path-style dependency with no registry source/checksum must fail"
    );

    let git_source = valid.replacen(
        CRATES_IO_REGISTRY_SOURCE,
        "git+https://example.invalid/serde?rev=deadbeef#deadbeef",
        1,
    );
    assert!(
        validate_probe_lock(&git_source).is_err(),
        "a Git dependency must fail even when it carries a checksum-shaped field"
    );

    let wrong_checksum = valid.replacen(
        REVIEWED_DIRECT_PACKAGES[0].checksum,
        "0000000000000000000000000000000000000000000000000000000000000000",
        1,
    );
    assert!(
        validate_probe_lock(&wrong_checksum).is_err(),
        "a changed direct dependency checksum must fail"
    );

    let widened_root =
        valid.replacen(" \"serde_json\",\n", " \"serde_json\",\n \"perl-parser-core\",\n", 1);
    assert_ne!(widened_root, valid);
    assert!(
        validate_probe_lock(&widened_root).is_err(),
        "a root dependency outside the reviewed set must fail even with reviewed packages intact"
    );
}

#[test]
fn jsonrpc_model_is_dependency_closed_and_rejects_indirect_perl_taxonomy()
-> Result<(), Box<dyn Error>> {
    let model = repo_root().join(JSONRPC_MODEL_PATH);
    if !model.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing selected JSON-RPC model: {}", model.display()),
        )
        .into());
    }

    let probe = tempfile::tempdir()?;
    let model_path = rust_path(&model);
    let neutral_source =
        format!("#![deny(warnings)]\n\n#[path = \"{model_path}\"]\npub mod jsonrpc;\n");
    write_probe(probe.path(), &neutral_source)?;

    let generated = generate_probe_lock(probe.path())?;
    if !generated.status.success() {
        return Err(io::Error::other(format!(
            "could not resolve the isolated offline probe lock:\n{}",
            output_text(&generated)
        ))
        .into());
    }
    let lock_path = probe.path().join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)?;
    validate_probe_lock(&lock).map_err(io::Error::other)?;

    let neutral = check_probe(probe.path())?;
    if !neutral.status.success() {
        return Err(io::Error::other(format!(
            "the real JSON-RPC model did not compile with the reviewed registry dependency closure:\n{}",
            output_text(&neutral)
        ))
        .into());
    }

    let indirect_taxonomy = format!(
        r#"#![allow(dead_code)]

#[path = "{model_path}"]
mod jsonrpc;

mod protocol {{
    pub use perl_parser_core::{{ErrorCategory, ErrorClass}};
}}

use jsonrpc::JsonRpcError;

impl protocol::ErrorClass for JsonRpcError {{
    fn error_class(&self) -> protocol::ErrorCategory {{
        protocol::ErrorCategory::Bug
    }}
}}
"#
    );
    write_probe(probe.path(), &indirect_taxonomy)?;

    let rejected = check_probe(probe.path())?;
    if rejected.status.success() {
        return Err(io::Error::other(
            "an indirect crate::protocol re-export restored Perl ErrorClass inside the neutral probe",
        )
        .into());
    }
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    if !stderr.contains("perl_parser_core") {
        return Err(io::Error::other(format!(
            "the indirect taxonomy probe failed for an unrelated reason:\n{}",
            output_text(&rejected)
        ))
        .into());
    }

    Ok(())
}
