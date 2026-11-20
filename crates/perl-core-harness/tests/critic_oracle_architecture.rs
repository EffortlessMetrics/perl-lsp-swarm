//! Architecture guard for repository-only Perl::Critic oracle process plans (#6987).
//!
//! The pinned oracle plan builder and `OwnerDomain::ExternalCritic` plans must
//! stay in the private harness; shipped runtime crates must not construct them.

use std::fs;
use std::path::{Path, PathBuf};

const PRODUCT_CRATE_ROOTS: &[&str] = &[
    "crates/perl-lsp-rs-core",
    "crates/perl-lsp-rs",
    "crates/perllsp",
    "crates/perl-lsp-perltidy",
];

const HARNESS_PLAN_BUILDER: &str = "build_critic_oracle_process_plan";
const HARNESS_OPERATION: &str = "perl-critic-conformance-oracle";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("source directory {} must be readable: {error}", dir.display())
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!("source directory entry in {} must be readable: {error}", dir.display())
        });
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn read_source(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("source file {} must be readable: {error}", path.display()))
}

#[test]
fn external_critic_process_plans_are_harness_only() {
    let root = workspace_root();
    let harness_root = root.join("crates/perl-core-harness");
    let mut harness_hits = Vec::new();
    let mut product_hits = Vec::new();

    for crate_root in PRODUCT_CRATE_ROOTS {
        let mut sources = Vec::new();
        collect_rust_sources(&root.join(crate_root), &mut sources);
        for path in sources {
            let source = read_source(&path);
            if source.contains("OwnerDomain::ExternalCritic")
                || source.contains(HARNESS_PLAN_BUILDER)
                || source.contains(HARNESS_OPERATION)
            {
                product_hits.push(path.strip_prefix(&root).unwrap_or(&path).display().to_string());
            }
        }
    }

    let mut harness_sources = Vec::new();
    collect_rust_sources(&harness_root, &mut harness_sources);
    for path in harness_sources {
        let source = read_source(&path);
        if source.contains(HARNESS_PLAN_BUILDER) || source.contains("OwnerDomain::ExternalCritic") {
            harness_hits.push(path.strip_prefix(&root).unwrap_or(&path).display().to_string());
        }
    }

    assert!(
        product_hits.is_empty(),
        "product/runtime crates must not reference the Perl::Critic conformance oracle plan: {product_hits:?}"
    );
    assert!(
        harness_hits.iter().any(|path| path.contains("critic_oracle_plan.rs")),
        "harness must own the oracle ProcessPlan builder, observed hits: {harness_hits:?}"
    );
}
