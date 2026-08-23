//! Structural containment for pull-diagnostic result identity (#7480).
//!
//! #7480 replaced content-only `md5(content)` result IDs on every pull
//! transport with a complete evaluation-and-projection subject composer.
//! These checks pin the structural recurrence guards:
//!
//! 1. No production pull-diagnostics source mints result IDs via `md5` or any
//!    other content-only digest helper.
//! 2. The only place `perl-lsp-rs` still depends on `md5` is recorded as an
//!    unrelated owned use (module-resolution/execute-command cache keys), not
//!    diagnostic report identity.

use std::path::PathBuf;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn crate_src_path(relative: &str) -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("src").join(relative)
}

fn read_source(relative: &str) -> TestResult<String> {
    let path = crate_src_path(relative);
    std::fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()).into())
}

/// Content-only digest calls are forbidden in the pull-identity surfaces.
///
/// If a future change genuinely needs a new digest authority there, it must go
/// through `report_identity.rs` and the domain-separated `ContentDigest`
/// authority — not an inline `md5`/hash helper.
#[test]
fn pull_identity_surfaces_contain_no_content_only_digest_helpers() -> TestResult<()> {
    let guarded = [
        "features/diagnostics/pull.rs",
        "features/diagnostics/report_identity.rs",
        "runtime/diagnostics.rs",
    ];

    for relative in guarded {
        let source = read_source(relative)?;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !trimmed.contains("md5::compute("),
                "{relative} must not mint pull result identity from content-only digests \
                 (#7480); found: {trimmed}"
            );
        }
    }

    Ok(())
}

/// `report_identity.rs` must compose through the repository's domain-separated
/// SHA-256 content-digest authority, never a hand-rolled hash or FNV mixer.
#[test]
fn pull_report_identity_composes_through_the_digest_authority() -> TestResult<()> {
    let source = read_source("features/diagnostics/report_identity.rs")?;

    assert!(
        source.contains("ContentDigest::of_bytes"),
        "result identity must fold through ContentDigest (#7480)"
    );

    for forbidden in ["DefaultHasher", "Hasher::new", "fnv", "md5::"] {
        assert!(
            !source.contains(forbidden),
            "free-form hashing ({forbidden}) must never become identity authority"
        );
    }

    Ok(())
}
