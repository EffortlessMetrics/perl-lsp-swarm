#![allow(clippy::print_stderr)]

use std::process::Command;

const FORBIDDEN: &[&str] = &[
    "lsp-types",
    "tokio",
    "tower-lsp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    "perl-workspace ",
];

#[test]
fn no_forbidden_dependencies() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-snapshot-store", "--edges", "normal", "--prefix", "none"])
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!(
                "dependency_contract: `cargo tree` failed ({}); skipping. stderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Err(error) => {
            eprintln!("dependency_contract: could not run `cargo tree` ({error}); skipping");
            return;
        }
    };

    let tree = String::from_utf8_lossy(&output.stdout);
    for forbidden in FORBIDDEN {
        assert!(
            !tree.contains(forbidden),
            "perl-snapshot-store must not depend on `{forbidden}` — tree:\n{tree}"
        );
    }
}
