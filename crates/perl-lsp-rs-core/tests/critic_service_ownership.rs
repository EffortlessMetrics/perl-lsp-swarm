//! Sole-owner inventory gate for the native critic service (#9062).
//!
//! After the #9062 cutover, exactly one production site may compose the
//! native critic pipeline (registry construction, candidate collection,
//! post-merge policy, canonical normalization): the service itself, plus the
//! settled #7475 seam that defines it and the registry implementation it
//! calls through. Every diagnostic/action transport must consume
//! [`NativeCriticService::analyze`] instead.
//!
//! This gate walks the production source trees of both owning packages and
//! fails closed if any composition entry point appears outside the
//! allowlist. A restored consumer-side pipeline — two paths snapshotting
//! configuration at different times, re-running semantic work to recover
//! findings, or flattening metadata differently — turns this red instead of
//! silently reintroducing the split the issue closed.

#![expect(
    clippy::panic,
    reason = "test-only barrier failure is a hard test error, not a production path"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Composition entry points reserved to the service and the seam modules.
const SERVICE_ONLY_COMPOSITION: [&str; 6] = [
    "native_finding_candidates(",
    "normalize_with_native_policy(",
    "NativeCriticPolicy::new(",
    "for_profile_with_config(",
    ".check_unfiltered(",
    "built_in_observation_candidates(",
];

/// Files allowed to contain composition entry points, with the reason each
/// allowance exists. Paths are relative to the workspace `crates/` directory
/// using forward slashes so the table reads identically on every platform.
const ALLOWED_SITES: [(&str, &str); 4] = [
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/service.rs",
        "#9062: the one protocol-neutral service",
    ),
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/semantic.rs",
        "#7475: the settled normalization/policy seam the service composes",
    ),
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/native/native_registry.rs",
        "the registry implementation itself (definition and internal check())",
    ),
    (
        "perl-lsp-rs/src/execute_command/provider.rs",
        "#6969 pending: the perl.runCritic command adapter cuts over separately",
    ),
];

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("source directory {} must be readable: {error}", dir.display())
    });
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Strip trailing test-only content so inline `#[cfg(test)]` modules do not
/// count as production call sites. Only ever shrinks the scanned surface.
fn production_portion(source: &str) -> &str {
    match source.rfind("#[cfg(test)]") {
        Some(index) => &source[..index],
        None => source,
    }
}

#[test]
fn the_native_critic_pipeline_is_composed_only_by_its_service() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut sources = Vec::new();
    let base = Path::new(manifest_dir);
    let crates_dir = match base.parent() {
        Some(parent) => parent.to_path_buf(),
        None => panic!("manifest dir {manifest_dir} must have a parent"),
    };
    for crate_src in [base.join("src"), crates_dir.join("perl-lsp-rs").join("src")] {
        assert!(
            crate_src.is_dir(),
            "owning package source tree {} must exist",
            crate_src.display()
        );
        collect_rust_sources(&crate_src, &mut sources);
    }
    assert!(
        sources.len() > 100,
        "the inventory must scan a real source tree; found only {} files",
        sources.len()
    );

    let mut violations = Vec::new();
    for path in &sources {
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        // Test-only module files are not production call sites.
        if file_name == "tests.rs" || file_name.starts_with("test_") {
            continue;
        }

        // Workspace-relative path: everything from the `crates` component on.
        let mut seen_crates = false;
        let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
        for component in path.components() {
            let raw = component.as_os_str();
            if seen_crates {
                parts.push(raw);
            } else if raw == "crates" {
                seen_crates = true;
                parts.push(raw);
            }
        }
        let Some(relative_path) =
            parts.split_first().map(|(_, tail)| tail.iter().collect::<PathBuf>())
        else {
            violations.push(format!(
                "{}: source file outside both owning crates cannot happen",
                path.display()
            ));
            continue;
        };
        let relative = relative_path.to_string_lossy().replace('\\', "/");

        let source = fs::read_to_string(path).unwrap_or_else(|error| {
            panic!("production source {} must be readable: {error}", path.display())
        });
        let production = production_portion(&source);
        for token in SERVICE_ONLY_COMPOSITION {
            if production.contains(token) {
                let allowance = ALLOWED_SITES.iter().find(|(allowed, _)| *allowed == relative);
                if allowance.is_none() {
                    violations.push(format!(
                        "{relative} composes `{token}` outside the native critic service (#9062)"
                    ));
                }
            }
        }
    }

    // The allowlist itself must stay honest: every entry still exists, so a
    // moved/renamed file cannot silently keep covering composition sites.
    for (allowed, reason) in ALLOWED_SITES {
        let absolute = crates_dir.join(allowed);
        assert!(absolute.is_file(), "allowlisted site {allowed} ({reason}) must exist");
    }

    assert!(
        violations.is_empty(),
        "native critic composition ownership violated:\n{}",
        violations.join("\n")
    );
}
