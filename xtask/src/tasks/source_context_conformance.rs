//! Run the #13246 completion source-context conformance matrix gates.

use color_eyre::eyre::{Context, Result};
use std::process::Command;

use crate::utils::project_root;

pub fn run_check() -> Result<()> {
    let root = project_root()?;
    let admitted = root.join("scripts/cargo-admitted");
    let steps: [(&str, &[&str]); 3] = [
        (
            "perl-parser-core canonical layer",
            &[
                "-p",
                "perl-parser-core",
                "--locked",
                "--test",
                "completion_source_context_conformance",
            ],
        ),
        (
            "perl-lsp-rs-core completion policy layer",
            &[
                "-p",
                "perl-lsp-rs-core",
                "--locked",
                "--",
                "completion_source_context_policy_matrix",
            ],
        ),
        (
            "perl-lsp-rs serialized completion layer",
            &["-p", "perl-lsp-rs", "--locked", "--test", "completion_source_context_conformance"],
        ),
    ];

    for (label, cargo_args) in steps {
        let status = Command::new(&admitted)
            .current_dir(&root)
            .arg("test")
            .args(cargo_args)
            .status()
            .with_context(|| format!("failed to spawn cargo-admitted for {label}"))?;
        if !status.success() {
            color_eyre::eyre::bail!("{label} failed with status {status}");
        }
    }

    Ok(())
}
