//! Compile-boundary proof for the neutral JSON-RPC wire model (#7611).
//!
//! The token-level dependency guard remains useful for broad, cheap scanning,
//! but it cannot prove that an allowed local import does not re-export a Perl
//! taxonomy. This test lets Rust compile the real `jsonrpc.rs` source in a
//! standalone crate whose complete dependency set is `serde` and `serde_json`.
//! The probe declares its own empty workspace so root workspace dependencies
//! and package metadata cannot make the selected source appear neutral. Its
//! exact dependency versions match the repository lockfile, so `--offline`
//! consumes artifacts already required to build this test rather than hidden
//! cache-only versions.
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

fn check_probe(probe_root: &Path) -> io::Result<Output> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    Command::new(cargo)
        .arg("check")
        .arg("--quiet")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(probe_root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(probe_root.join("target"))
        .output()
}

fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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

    let neutral = check_probe(probe.path())?;
    if !neutral.status.success() {
        return Err(io::Error::other(format!(
            "the real JSON-RPC model did not compile with only serde and serde_json:\n{}",
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
