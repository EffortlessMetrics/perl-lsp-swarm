//! Checked inventory of direct `git merge-base` consumers inside `xtask`.
//!
//! Issue #10304 added the shared read-only ancestry authority
//! (`xtask::git_ancestry`) because a failed or empty `git merge-base` is not
//! proof of unrelated history in a shallow, partial, or object-incomplete
//! checkout, and migrated the RIPR committed-diff seam onto it.
//!
//! The remaining consumers are recorded here rather than migrated in the same
//! candidate. This inventory is the "checked migration inventory" #10304 asks
//! for: it fails when a new direct consumer appears without a disposition, and
//! when a recorded consumer stops invoking `merge-base` and the row goes stale.
//! It does not duplicate the classifier.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// How one direct `git merge-base` consumer relates to the shared authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Disposition {
    /// `xtask::git_ancestry` itself — the authority that owns the interpretation.
    Authority,
    /// Uses a successful merge base only to compute a range or boundary, and
    /// propagates failure as an error instead of a history verdict.
    RangeOnly,
    /// Calls `merge-base --is-ancestor` and maps exit 1 to an affirmative
    /// "not an ancestor" conclusion without a shallow/partial guard. A shallow
    /// checkout holding a present-but-disconnected commit reports exit 1 where a
    /// complete clone reports exit 0, so these conclusions can be false.
    IsAncestorPendingMigration,
    /// Test code, a read-only command allowlist, or a comment; no ancestry
    /// decision is made here.
    TestOrAllowlistOnly,
}

struct ConsumerRow {
    path: &'static str,
    disposition: Disposition,
    /// Issue that owns the remaining migration, when one is still open.
    successor: Option<&'static str>,
}

/// Every file under `xtask/src` that may invoke `git merge-base` directly.
///
/// `xtask/src/tasks/ripr_evidence.rs` is deliberately absent: #10304 migrated it
/// onto `xtask::git_ancestry`, so it no longer invokes `merge-base` at all. If it
/// reappears here, the migration has regressed.
const INVENTORY: &[ConsumerRow] = &[
    ConsumerRow {
        path: "src/git_ancestry.rs",
        disposition: Disposition::Authority,
        successor: None,
    },
    ConsumerRow {
        path: "src/bin/action-pin-provenance.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/changelog.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/ci_contract.rs",
        disposition: Disposition::RangeOnly,
        successor: None,
    },
    ConsumerRow {
        path: "src/tasks/ci_subject.rs",
        disposition: Disposition::RangeOnly,
        successor: None,
    },
    ConsumerRow {
        path: "src/tasks/file_policy.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/merge_integration.rs",
        disposition: Disposition::RangeOnly,
        successor: None,
    },
    ConsumerRow {
        path: "src/tasks/merge_ready.rs",
        disposition: Disposition::RangeOnly,
        successor: None,
    },
    ConsumerRow {
        path: "src/tasks/module_train_live.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/module_train_live_tests.rs",
        disposition: Disposition::TestOrAllowlistOnly,
        successor: None,
    },
    ConsumerRow {
        path: "src/tasks/pr_close_proof.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/sync_divergence.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
    ConsumerRow {
        path: "src/tasks/workflows.rs",
        disposition: Disposition::IsAncestorPendingMigration,
        successor: Some("#14557"),
    },
];

fn xtask_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Files under `xtask/src` that pass `merge-base` to git as an argument.
fn observed_consumers() -> Vec<String> {
    let root = xtask_root().join("src");
    let mut found = Vec::new();
    collect_rust_sources(&root, &mut found);
    let mut consumers = found
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path).is_ok_and(|source| source.contains("\"merge-base\""))
        })
        .map(|path| {
            path.strip_prefix(xtask_root()).unwrap_or(&path).to_string_lossy().replace('\\', "/")
        })
        .collect::<Vec<_>>();
    consumers.sort();
    consumers
}

fn collect_rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn every_direct_merge_base_consumer_has_a_disposition() {
    let observed: BTreeSet<String> = observed_consumers().into_iter().collect();
    let recorded: BTreeSet<String> = INVENTORY.iter().map(|row| row.path.to_string()).collect();

    let undeclared: Vec<&String> = observed.difference(&recorded).collect();
    assert!(
        undeclared.is_empty(),
        "new direct `git merge-base` consumers must consume xtask::git_ancestry or be given an \
         inventory disposition (#10304): {undeclared:?}"
    );
}

#[test]
fn inventory_rows_do_not_go_stale() {
    let observed: BTreeSet<String> = observed_consumers().into_iter().collect();
    let recorded: BTreeSet<String> = INVENTORY.iter().map(|row| row.path.to_string()).collect();

    let stale: Vec<&String> = recorded.difference(&observed).collect();
    assert!(
        stale.is_empty(),
        "these inventory rows no longer invoke `git merge-base` and must be removed (#10304): \
         {stale:?}"
    );
}

/// The migrated seam must stay migrated: RIPR consumes the shared authority and
/// must not reacquire a private `merge-base` interpretation.
#[test]
fn ripr_evidence_does_not_reacquire_a_private_merge_base_interpretation() -> std::io::Result<()> {
    let source = fs::read_to_string(xtask_root().join("src/tasks/ripr_evidence.rs"))?;
    // Scan production code only. The fixtures in `mod tests` legitimately drive
    // real shallow repositories to prove the migrated seam.
    let production = source
        .split_once("\n#[cfg(test)]\nmod tests {")
        .map_or(source.as_str(), |(before, _)| before);

    assert!(
        !production.contains("\"merge-base\""),
        "ripr_evidence.rs must reach ancestry through xtask::git_ancestry, not `git merge-base` \
         directly (#10304)"
    );
    assert!(
        production.contains("classify_ancestry"),
        "ripr_evidence.rs must consume the shared ancestry authority (#10304)"
    );
    assert!(
        !production.contains("--is-shallow-repository"),
        "ripr_evidence.rs must not run a private shallow probe; shallow interpretation belongs to \
         xtask::git_ancestry (#10304)"
    );
    Ok(())
}

/// Every consumer still pending migration must carry an explicit successor, so
/// the residual risk stays attributable instead of dissolving into the inventory.
#[test]
fn pending_migrations_name_a_successor() {
    for row in INVENTORY {
        if row.disposition == Disposition::IsAncestorPendingMigration {
            assert!(
                row.successor.is_some(),
                "{} maps `--is-ancestor` exit 1 to a history conclusion and must name a successor \
                 (#10304)",
                row.path
            );
        }
    }
}
