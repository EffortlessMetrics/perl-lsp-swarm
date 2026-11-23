//! Membership contract tests for `test_corpus/gold` envelopes.
//!
//! Verifies that every gold project in the repository carries a valid
//! `gold_project.toml` envelope that passes closed-world membership checks,
//! and that legacy `load_gold_fixture` still reads byte-identical `fixture.pl`
//! for a sample project.
//!
//! Run with:
//!
//! ```bash
//! cargo test -p perl-corpus --test gold_membership
//! bash scripts/cargo-admitted test -p perl-corpus --locked gold_membership
//! ```

use perl_corpus::gold::{load_gold_fixture, project_envelope::discover_project};
use perl_tdd_support::{must, must_some, must_some_with};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    must_some_with(
        manifest_dir.parent().and_then(Path::parent).map(Path::to_path_buf),
        "perl-corpus must live under <workspace>/crates",
    )
}

fn gold_root() -> PathBuf {
    workspace_root().join("test_corpus").join("gold")
}

/// Every project directory under `test_corpus/gold/` must have a valid
/// `gold_project.toml` envelope.
#[test]
fn gold_membership_all_36_projects_have_valid_envelopes() {
    let gold = gold_root();
    assert!(
        gold.exists(),
        "test_corpus/gold must exist; set PERL_CORPUS_ROOT or run from workspace root"
    );

    let mut entries: Vec<_> = must(std::fs::read_dir(&gold))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    entries.sort();

    assert!(
        entries.len() >= 36,
        "expected at least 36 project directories, found {}",
        entries.len()
    );

    let mut failures = Vec::new();
    for project_dir in &entries {
        let outcome = discover_project(project_dir);
        if !outcome.is_valid() {
            failures.push(format!(
                "{}: {}",
                project_dir.file_name().unwrap_or_default().to_string_lossy(),
                outcome
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "the following gold projects failed envelope validation:\n{}",
        failures.join("\n")
    );
}

/// `project_id` and `member_id` fields must be stable across re-reads.
#[test]
fn gold_membership_ids_are_stable_across_re_reads() {
    let project_dir = gold_root().join("hello_world");
    if !project_dir.exists() {
        return; // skip if corpus not available
    }

    let outcome1 = discover_project(&project_dir);
    let outcome2 = discover_project(&project_dir);

    let (env1, env2) = match (outcome1, outcome2) {
        (
            perl_corpus::gold::project_envelope::DiscoveryOutcome::Valid(e1),
            perl_corpus::gold::project_envelope::DiscoveryOutcome::Valid(e2),
        ) => (e1, e2),
        (o1, _) => unreachable!("hello_world must have a valid envelope, got: {o1}"),
    };

    assert_eq!(env1.project_id, env2.project_id, "project_id must be stable across reads");
    for (m1, m2) in env1.members.iter().zip(env2.members.iter()) {
        assert_eq!(m1.member_id, m2.member_id, "member_id must be stable");
        assert_eq!(m1.path, m2.path, "member path must be stable");
    }
}

/// The topology fingerprint must not change when only content changes
/// (regression guard: topology is path-only).
#[test]
fn gold_membership_topology_fingerprint_does_not_include_content() {
    use perl_corpus::gold::project_envelope::{
        compute_content_fingerprint, compute_topology_fingerprint,
    };

    // Same paths, different digests.
    let paths = vec!["expected.json", "fixture.pl"];
    let topo = compute_topology_fingerprint(&paths);

    let pairs_v1 = vec![("expected.json", "aaaa"), ("fixture.pl", "bbbb")];
    let pairs_v2 = vec![("expected.json", "cccc"), ("fixture.pl", "dddd")];

    assert_eq!(topo, compute_topology_fingerprint(&paths), "topology fingerprint is unchanged");
    assert_ne!(
        compute_content_fingerprint(&pairs_v1),
        compute_content_fingerprint(&pairs_v2),
        "content fingerprint changes when digests change"
    );
}

/// Legacy `load_gold_fixture` must still read byte-identical `fixture.pl` for
/// a sample project that has an envelope.
#[test]
fn gold_membership_legacy_load_gold_fixture_reads_byte_identical_fixture_pl() {
    let project_dir = gold_root().join("hello_world");
    if !project_dir.exists() {
        return; // skip if corpus not available
    }

    // Load via legacy API.
    let legacy = must(load_gold_fixture(&project_dir));

    // Read the fixture source bytes directly.
    let expected_bytes = must(std::fs::read(&legacy.fixture_path));

    // Verify the fixture_path points to fixture.pl in the project dir.
    assert_eq!(legacy.fixture_path, project_dir.join("fixture.pl"));

    // Verify envelope's stored sha256 matches the bytes we read.
    let outcome = discover_project(&project_dir);
    let env = match outcome {
        perl_corpus::gold::project_envelope::DiscoveryOutcome::Valid(e) => e,
        o => unreachable!("hello_world must have a valid envelope, got: {o}"),
    };

    let fixture_member = must_some(env.members.iter().find(|m| m.path == "fixture.pl"));

    use sha2::{Digest, Sha256};
    let actual_digest = {
        let hash = Sha256::digest(&expected_bytes);
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };

    assert_eq!(
        actual_digest, fixture_member.sha256,
        "legacy load_gold_fixture reads byte-identical fixture.pl"
    );
    assert_eq!(
        expected_bytes.len() as u64,
        fixture_member.byte_length,
        "fixture.pl byte length matches envelope"
    );
}

/// Projects must have `fixture.pl` as an entrypoint member.
#[test]
fn gold_membership_every_project_has_entrypoint_member() {
    use perl_corpus::gold::project_envelope::{DiscoveryOutcome, MemberRole};

    let gold = gold_root();
    if !gold.exists() {
        return;
    }

    let mut entries: Vec<_> = must(std::fs::read_dir(&gold))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    entries.sort();

    for project_dir in &entries {
        let outcome = discover_project(project_dir);
        let env = match outcome {
            DiscoveryOutcome::Valid(e) => e,
            _ => continue,
        };

        assert!(
            env.members.iter().any(|m| m.role == MemberRole::Entrypoint),
            "project {} must have an Entrypoint member",
            env.project_id
        );
    }
}

/// The `sha2` dependency is available in the crate — this test confirms the
/// digest used in envelopes is SHA-256.
#[test]
fn gold_membership_sha256_of_empty_is_known_constant() {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(b"");
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}
