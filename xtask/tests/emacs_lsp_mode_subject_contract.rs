//! Discriminating contract tests for the external lsp-mode subject rows of
//! #11746 (released MELPA Stable lsp-mode and pinned upstream-source
//! lsp-mode).
//!
//! The first seven tests are the issue's first falsifiers, in the order the
//! body lists them:
//!
//! 1. released subject omits package/archive identity -> reject;
//! 2. source uses `main`, `HEAD`, or a mutable tag/alias -> reject;
//! 3. source header version is mistaken for released package identity ->
//!    reject;
//! 4. same version with different package bytes survives -> reject;
//! 5. minimum-Emacs/dependency drift is ignored -> reject;
//! 6. ambient package/native-comp copy satisfies the exact subject ->
//!    reject;
//! 7. manifest intent is treated as runtime selected-client proof ->
//!    reject.
//!
//! The remaining tests pin the positive contract: both subjects materialize
//! through the #11744 resolver binding their exact identities, released and
//! source states stay non-interchangeable, the archive digest (not the
//! version) carries the cache identity, the checked manifest pins the
//! audited digests, and the registry agrees with the manifest. The run-plan
//! boundary check proves the archive constraint is enforced before any
//! launch-shaped step: repacked archive bytes are refused by the resolver
//! before the (not yet existing) lsp-mode adapter is even digested. No
//! client-selection, semantic journey, upstream acceptance, or public
//! support claim is proven here.

// Plain #[test] functions assert through the standard panic-on-failure
// idiom; these tests are proof, not production paths. `expect` (not
// `allow`) so Clippy flags the suppression once the idiom moves on.
#![expect(clippy::expect_used, clippy::panic)]

use anyhow::{Context, Result, ensure};
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_host_run::{self, EmacsClientSubject, build_client_subject_run_plan};
use xtask::emacs_subject_manifest::{
    CACHE_ENTRY_FILE, ExternalPackageIdentity, MaterializationMethod, ResolveFailure,
    ResolveRequest, SourceTreeIdentity, SubjectClientKind, SubjectManifest, SubjectRejection,
    SubjectRow, resolve,
};

fn workspace_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live directly under the workspace root")?
        .to_path_buf())
}

fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut identity = String::from("sha256:");
    for byte in hasher.finalize() {
        identity.push_str(&format!("{byte:02x}"));
    }
    identity
}

/// Fixture bytes standing in for the audited MELPA Stable archive's
/// `lsp-mode.el` and the audited upstream-tree `lsp-mode.el`. The audited
/// pair carries different version headers (10.0.0 released, 10.0.1 source)
/// AND different bytes; the fixtures keep both differences so a header
/// resemblance is never what binds either subject.
const RELEASED_LSP_MODE_BYTES: &[u8] =
    b";; lsp-mode.el --- released MELPA Stable 10.0.0 client (fixture)\n;; Version: 10.0.0\n";
const SOURCE_LSP_MODE_BYTES: &[u8] =
    b";; lsp-mode.el --- upstream source client (fixture)\n;; Version: 10.0.1\n";
const RELEASED_ARCHIVE_BYTES: &[u8] = b"fixture MELPA Stable archive bytes for lsp-mode 10.0.0";
const REPACKED_ARCHIVE_BYTES: &[u8] =
    b"repacked archive bytes that still claim lsp-mode 10.0.0 (different bytes)";

const RELEASED_SOURCE_COMMIT: &str = "913a6c07f163205cb568bc68d7dfe677dbc358ab";
const SOURCE_TREE_COMMIT: &str = "6bfc593d7b1bc0dd656f09ffce52cc085ebced05";
const SOURCE_TREE_SHA1: &str = "b9111a657fe1376f92d203ba4951868fb0fa3f57";

const RELEASED_ID: &str = "released_lsp_mode_melpa_stable_10_0_0";
const SOURCE_ID: &str = "source_lsp_mode_github_6bfc593";

fn fixture_external_package(
    archive_sha256: String,
    minimum_emacs: &str,
) -> ExternalPackageIdentity {
    ExternalPackageIdentity {
        archive_url: "https://stable.melpa.org/packages/lsp-mode-fixture.tar".to_string(),
        archive_sha256,
        attested_source_commit: RELEASED_SOURCE_COMMIT.to_string(),
        package_requires: vec![
            format!("emacs {minimum_emacs}"),
            "dash 2.18.0".to_string(),
            "eldoc 1.11".to_string(),
        ],
        minimum_emacs: minimum_emacs.to_string(),
        checksum_disposition: "melpa_stable_archive_sha256_at_audit_time".to_string(),
    }
}

fn released_row(client_sha256: String, archive_sha256: String, minimum_emacs: &str) -> SubjectRow {
    SubjectRow {
        subject_id: RELEASED_ID.to_string(),
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::Released,
        emacs_release_tag: RELEASED_SOURCE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "10.0.0".to_string(),
        client_source_relative_path: "lsp-mode.el".to_string(),
        client_source_sha256: client_sha256,
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["lsp-mode.el".to_string()],
        external_package: Some(fixture_external_package(archive_sha256.clone(), minimum_emacs)),
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://stable.melpa.org/packages/lsp-mode-fixture.tar".to_string(),
            gnu_tarball_sha256: archive_sha256,
            observed_client_version_header: "10.0.0".to_string(),
        },
    }
}

fn source_row(client_sha256: String) -> SubjectRow {
    SubjectRow {
        subject_id: SOURCE_ID.to_string(),
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: SOURCE_TREE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "10.0.1".to_string(),
        client_source_relative_path: "lsp-mode.el".to_string(),
        client_source_sha256: client_sha256.clone(),
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["lsp-mode.el".to_string()],
        external_package: None,
        source_tree: Some(SourceTreeIdentity {
            source_repo_url: "https://github.com/emacs-lsp/lsp-mode".to_string(),
            commit: SOURCE_TREE_COMMIT.to_string(),
            tree_sha1: SOURCE_TREE_SHA1.to_string(),
        }),
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url:
                "https://raw.githubusercontent.com/emacs-lsp/lsp-mode/fixture/lsp-mode.el"
                    .to_string(),
            gnu_tarball_sha256: client_sha256,
            observed_client_version_header: "10.0.1".to_string(),
        },
    }
}

fn fixture_manifest() -> SubjectManifest {
    SubjectManifest {
        schema_version: xtask::emacs_subject_manifest::MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![
            released_row(
                sha256_of(RELEASED_LSP_MODE_BYTES),
                sha256_of(RELEASED_ARCHIVE_BYTES),
                "28.1",
            ),
            source_row(sha256_of(SOURCE_LSP_MODE_BYTES)),
        ],
    }
}

/// A fake exact Emacs executable: `<root>/bin/emacs`. External subjects
/// never search the installation tree.
fn fixture_emacs(root: &Path) -> Result<PathBuf> {
    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;
    let emacs = bin.join("emacs");
    fs::write(&emacs, b"fake exact emacs 30.1 executable")?;
    Ok(emacs)
}

/// Materialize a bounded external input directory: the exact client file
/// `<dir>/lsp-mode.el` and, when requested, the exact archive
/// `<dir>/lsp-mode-10.0.0.tar`.
fn fixture_input_dir(
    root: &Path,
    client_bytes: &[u8],
    archive_bytes: Option<&[u8]>,
) -> Result<PathBuf> {
    let dir = root.join("materialized/lsp-mode-exact-input");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("lsp-mode.el"), client_bytes)?;
    if let Some(bytes) = archive_bytes {
        fs::write(dir.join("lsp-mode-10.0.0.tar"), bytes)?;
    }
    Ok(dir)
}

fn default_request<'a>(
    emacs_executable: &'a Path,
    client_source: Option<&'a Path>,
    client_package: Option<&'a Path>,
    cache_root: &'a Path,
) -> ResolveRequest<'a> {
    ResolveRequest {
        emacs_executable,
        client_source,
        client_package,
        cache_root,
        probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
    }
}

fn rejection_of(failure: &ResolveFailure) -> &SubjectRejection {
    match failure {
        ResolveFailure::Rejected(rejection) => rejection,
        ResolveFailure::Instrument(message) => {
            panic!("expected a typed rejection, got an instrument failure: {message}")
        }
    }
}

// ---------------------------------------------------------------------------
// First falsifiers (issue order)
// ---------------------------------------------------------------------------

/// Falsifier 1: a released lsp-mode subject without its exact
/// package/archive identity is unavailable, and a released row that
/// declares none is an invalid manifest.
#[test]
fn released_subject_without_package_identity_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, None).expect("input dir");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&input.join("lsp-mode.el")),
            None,
            &root.path().join("cache"),
        ),
    )
    .expect_err("a released lsp-mode subject requires the exact package archive input");
    match rejection_of(&failure) {
        SubjectRejection::UnavailableSubject { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains("package archive"),
                "the reason must name the missing package/archive identity: {reason}"
            );
        }
        other => panic!("expected UnavailableSubject, got {other:?}"),
    }

    let mut no_package = fixture_manifest();
    no_package.subjects[0].external_package = None;
    let rejection =
        no_package.validate().expect_err("a released row must declare package identity");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("package/archive identity"),
            "the reason must name the released-identity rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 2: `main`, `HEAD`, and mutable aliases never pin source bytes,
/// and a tag that disagrees with the pinned tree is not an exact tree.
#[test]
fn floating_source_refs_are_rejected() {
    for floating in ["main", "HEAD", "trunk", "latest", "master"] {
        let mut manifest = fixture_manifest();
        manifest.subjects[1].emacs_release_tag = floating.to_string();
        let rejection =
            manifest.validate().expect_err("a floating source ref must fail validation");
        match &rejection {
            SubjectRejection::InvalidManifest { reason } => assert!(
                reason.contains("floating") || reason.contains("commit pin"),
                "the reason must name the ref rule: {reason}"
            ),
            other => panic!("expected InvalidManifest, got {other:?}"),
        }
    }
    let mut aliased = fixture_manifest();
    aliased.subjects[1].source_tree = Some(SourceTreeIdentity {
        source_repo_url: "https://github.com/emacs-lsp/lsp-mode".to_string(),
        commit: "0f4aa7b3d4b3b7f2f83a2b7a694af4a3d2c1b0a9".to_string(),
        tree_sha1: SOURCE_TREE_SHA1.to_string(),
    });
    let rejection = aliased.validate().expect_err("tag/tree disagreement must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("commit pin"),
            "the reason must name the tag/tree coherence rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 3: the source tree's header version never substitutes for
/// released package identity — the bytes bind the subject in both
/// directions.
#[test]
fn source_header_version_is_not_released_package_identity() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), SOURCE_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))
        .expect("input dir");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&input.join("lsp-mode.el")),
            Some(&input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect_err("source-tree bytes must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains(&sha256_of(SOURCE_LSP_MODE_BYTES))
                    && reason.contains(&sha256_of(RELEASED_LSP_MODE_BYTES)),
                "the reason must name the observed and pinned digests: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }

    let released_input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, None).expect("released input");
    let failure = resolve(
        &manifest,
        SOURCE_ID,
        &default_request(
            &emacs,
            Some(&released_input.join("lsp-mode.el")),
            None,
            &root.path().join("cache-2"),
        ),
    )
    .expect_err("released bytes must not satisfy the source subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, .. } => {
            assert_eq!(subject_id, SOURCE_ID);
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }

    // Relabeling the source row as released without package identity is
    // the forbidden source-as-release labeling.
    let mut mislabeled = fixture_manifest();
    mislabeled.subjects[1].source_state = ClientSourceState::Released;
    mislabeled.subjects[1].external_package = None;
    let rejection = mislabeled.validate().expect_err("a mislabeled source row must not validate");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("package/archive identity"),
            "the reason must name the released-identity rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 4: the same version over different package bytes is a
/// different subject; the archive digest is validated before anything is
/// loadable.
#[test]
fn same_version_different_package_bytes_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(REPACKED_ARCHIVE_BYTES))
            .expect("input dir");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&input.join("lsp-mode.el")),
            Some(&input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect_err("repacked archive bytes must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains(&sha256_of(REPACKED_ARCHIVE_BYTES))
                    && reason.contains(&sha256_of(RELEASED_ARCHIVE_BYTES)),
                "the reason must name the observed and declared archive digests: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    assert!(
        !root.path().join("cache").exists(),
        "a rejected resolution must not write cache state"
    );
}

/// Falsifier 5: minimum-Emacs and dependency drift is refused, never
/// silently compatible.
#[test]
fn emacs_compatibility_drift_is_rejected() {
    // The pinned host token below the declared minimum Emacs floor.
    let mut drifted = fixture_manifest();
    drifted.subjects[0].emacs_version_token = "27.1".to_string();
    let rejection = drifted.validate().expect_err("host-token drift must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("below the declared minimum Emacs"),
            "the reason must name the compatibility floor: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }

    // The declared minimum disagreeing with the emacs dependency entry.
    let mut mismatched = fixture_manifest();
    mismatched.subjects[0].external_package = Some(ExternalPackageIdentity {
        archive_url: "https://stable.melpa.org/packages/lsp-mode-fixture.tar".to_string(),
        archive_sha256: sha256_of(RELEASED_ARCHIVE_BYTES),
        attested_source_commit: RELEASED_SOURCE_COMMIT.to_string(),
        package_requires: vec!["emacs 29.1".to_string(), "dash 2.18.0".to_string()],
        minimum_emacs: "28.1".to_string(),
        checksum_disposition: "melpa_stable_archive_sha256_at_audit_time".to_string(),
    });
    let rejection = mismatched.validate().expect_err("requires/minimum drift must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("minimum_emacs"),
            "the reason must name the requires/minimum coherence rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }

    // A probed host without the pinned token stays an explicit
    // incompatibility for lsp-mode subjects too.
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))
            .expect("input dir");
    let request = ResolveRequest {
        emacs_executable: &emacs,
        client_source: Some(&input.join("lsp-mode.el")),
        client_package: Some(&input.join("lsp-mode-10.0.0.tar")),
        cache_root: &root.path().join("cache"),
        probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
    };
    let failure = resolve(&fixture_manifest(), RELEASED_ID, &request)
        .expect_err("a host without the pinned token is incompatible");
    match rejection_of(&failure) {
        SubjectRejection::IncompatibleSubject { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains("29.4") && reason.contains("30.1"),
                "the reason must name observed and pinned tokens: {reason}"
            );
        }
        other => panic!("expected IncompatibleSubject, got {other:?}"),
    }
}

/// Falsifier 6: an ambient package-layout copy — even byte-identical —
/// cannot satisfy an exact subject.
#[test]
fn ambient_package_copy_cannot_satisfy_lsp_mode_subjects() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let ambient = root.path().join("home/.emacs.d/elpa/lsp-mode-10.0.0/lsp-mode.el");
    fs::create_dir_all(ambient.parent().expect("parent")).expect("elpa dir");
    fs::write(&ambient, RELEASED_LSP_MODE_BYTES).expect("ambient copy");
    let archive = root.path().join("input/lsp-mode-10.0.0.tar");
    fs::create_dir_all(archive.parent().expect("parent")).expect("input dir");
    fs::write(&archive, RELEASED_ARCHIVE_BYTES).expect("archive");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(&emacs, Some(&ambient), Some(&archive), &root.path().join("cache")),
    )
    .expect_err("an ambient ELPA copy must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains("elpa"),
                "the reason must name the package-layout marker: {reason}"
            );
        }
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }

    let ambient_source = root.path().join("home/.emacs.d/elpa/lsp-mode-10.0.1/lsp-mode.el");
    fs::create_dir_all(ambient_source.parent().expect("source parent")).expect("elpa source dir");
    fs::write(&ambient_source, SOURCE_LSP_MODE_BYTES).expect("ambient source copy");
    let failure = resolve(
        &manifest,
        SOURCE_ID,
        &default_request(&emacs, Some(&ambient_source), None, &root.path().join("cache-2")),
    )
    .expect_err("an ambient ELPA copy must not satisfy the source subject");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { subject_id, reason } => {
            assert_eq!(subject_id, SOURCE_ID);
            assert!(reason.contains("elpa"), "the reason must name the marker: {reason}");
        }
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }
}

/// Falsifier 7: manifest intent is never runtime selected-client proof.
/// The cache record carries intended input only, and a doctored record
/// claiming a runtime observation fails closed.
#[test]
fn manifest_intent_is_never_runtime_selected_client_proof() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))
            .expect("input dir");
    let manifest = fixture_manifest();
    let resolved = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&input.join("lsp-mode.el")),
            Some(&input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect("the released subject resolves");
    let record_path = resolved.cache_entry.join(CACHE_ENTRY_FILE);
    let record = fs::read_to_string(&record_path).expect("cache record");
    assert!(
        record.contains("intended input"),
        "the record must carry the intended-input boundary: {record}"
    );
    assert!(
        !record.contains("loaded") && !record.contains("runtime_pass"),
        "the record vocabulary must not claim any runtime load: {record}"
    );

    let doctored: serde_json::Value = serde_json::from_str(&record).expect("record json");
    let mut object = doctored.as_object().expect("record object").clone();
    object.insert(
        "runtime_loaded_sha256".to_string(),
        serde_json::json!(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        ),
    );
    fs::write(&record_path, serde_json::to_vec_pretty(&object).expect("serialize"))
        .expect("doctored record");
    let failure = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&input.join("lsp-mode.el")),
            Some(&input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect_err("a doctored record claiming runtime observation must fail closed");
    match &failure {
        ResolveFailure::Instrument(message) => assert!(
            message.contains("runtime_loaded_sha256") || message.contains("refuses to parse"),
            "the doctored record must fail as a parse refusal naming the rejected claim: {message}"
        ),
        ResolveFailure::Rejected(rejection) => {
            panic!("expected an instrument failure, got a rejection: {rejection}")
        }
    }
}

// ---------------------------------------------------------------------------
// Positive contract
// ---------------------------------------------------------------------------

/// The released lsp-mode subject resolves binding the exact archive and
/// loaded-file identities and a deterministic cache entry.
#[test]
fn released_subject_binds_exact_archive_and_file_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let client_path = input.join("lsp-mode.el");
    let archive_path = input.join("lsp-mode-10.0.0.tar");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(&emacs, Some(&client_path), Some(&archive_path), &cache),
    )?;
    ensure!(!resolved.reused_cache, "a fresh cache must materialize");
    ensure!(
        resolved.client.client_id == RELEASED_ID
            && resolved.client.kind == emacs_host_run::emacs_host_runner::EmacsClientKind::LspMode
            && resolved.client.source_state == ClientSourceState::Released
    );
    ensure!(
        resolved.client.source_sha256 == sha256_of(RELEASED_LSP_MODE_BYTES)
            && resolved.client.package_sha256.as_deref()
                == Some(sha256_of(RELEASED_ARCHIVE_BYTES).as_str())
    );
    ensure!(
        resolved.client.source_ref == RELEASED_SOURCE_COMMIT,
        "the released subject's source ref is the archive-attested commit"
    );
    ensure!(
        resolved.emacs_build_sha256 == sha256_of(b"fake exact emacs 30.1 executable"),
        "the exact host build digest is bound at resolution"
    );
    let again = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(&emacs, Some(&client_path), Some(&archive_path), &cache),
    )?;
    ensure!(again.reused_cache && again.cache_key == resolved.cache_key);
    Ok(())
}

/// The source lsp-mode subject materializes from the exact tree file under
/// its immutable commit pin, with no package identity anywhere.
#[test]
fn source_subject_materializes_from_the_exact_tree() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let input = fixture_input_dir(root.path(), SOURCE_LSP_MODE_BYTES, None)?;
    let client_path = input.join("lsp-mode.el");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved =
        resolve(&manifest, SOURCE_ID, &default_request(&emacs, Some(&client_path), None, &cache))?;
    ensure!(
        resolved.client.source_state == ClientSourceState::UpstreamSource
            && resolved.client.source_ref == SOURCE_TREE_COMMIT
            && resolved.client.source_sha256 == sha256_of(SOURCE_LSP_MODE_BYTES)
            && resolved.client.package_sha256.is_none()
    );
    ensure!(resolved.client_package.is_none());
    let again =
        resolve(&manifest, SOURCE_ID, &default_request(&emacs, Some(&client_path), None, &cache))?;
    ensure!(again.reused_cache && again.cache_key == resolved.cache_key);
    Ok(())
}

/// Released and source lsp-mode subjects never share an identity, and
/// package input cannot satisfy a source subject.
#[test]
fn released_and_source_subjects_are_not_interchangeable() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let released_input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let source_input = fixture_input_dir(&root.path().join("src"), SOURCE_LSP_MODE_BYTES, None)?;
    let manifest = fixture_manifest();
    let released = resolve(
        &manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&released_input.join("lsp-mode.el")),
            Some(&released_input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache-released"),
        ),
    )?;
    let source = resolve(
        &manifest,
        SOURCE_ID,
        &default_request(
            &emacs,
            Some(&source_input.join("lsp-mode.el")),
            None,
            &root.path().join("cache-source"),
        ),
    )?;
    ensure!(
        released.cache_key != source.cache_key,
        "released and source subjects must never share a cache key"
    );
    ensure!(
        released.client.version != source.client.version
            || released.client.source_sha256 != source.client.source_sha256,
        "the audited pair differs in header, bytes, or both"
    );

    let failure = resolve(
        &manifest,
        SOURCE_ID,
        &default_request(
            &emacs,
            Some(&source_input.join("lsp-mode.el")),
            Some(&released_input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache-mix"),
        ),
    )
    .expect_err("package input cannot satisfy a source subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, SOURCE_ID);
            assert!(
                reason.contains("non-interchangeable"),
                "the reason must name the non-interchangeability rule: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    Ok(())
}

/// A re-audited archive digest under the same version and loaded-file
/// digest makes the old cache entry stale and produces a different key:
/// the archive bytes, not the version, carry the cache identity.
#[test]
fn cache_keys_bind_the_archive_digest_not_the_version_alone() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let original_input =
        fixture_input_dir(root.path(), RELEASED_LSP_MODE_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let repacked_root = root.path().join("repacked");
    let repacked_input =
        fixture_input_dir(&repacked_root, RELEASED_LSP_MODE_BYTES, Some(REPACKED_ARCHIVE_BYTES))?;

    let original_manifest = fixture_manifest();
    let original = resolve(
        &original_manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&original_input.join("lsp-mode.el")),
            Some(&original_input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache-shared"),
        ),
    )?;

    let mut repacked_manifest = fixture_manifest();
    repacked_manifest.subjects[0].external_package =
        Some(fixture_external_package(sha256_of(REPACKED_ARCHIVE_BYTES), "28.1"));
    repacked_manifest.subjects[0].digest_audit.gnu_tarball_sha256 =
        sha256_of(REPACKED_ARCHIVE_BYTES);
    repacked_manifest.validate().expect("the re-audited row must itself be a valid manifest");

    let failure = resolve(
        &repacked_manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&repacked_input.join("lsp-mode.el")),
            Some(&repacked_input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache-shared"),
        ),
    )
    .expect_err("the immutable cache must refuse the re-audited identity under the old entry");
    match rejection_of(&failure) {
        SubjectRejection::StaleCacheEntry { subject_id, reason, .. } => {
            assert_eq!(subject_id, RELEASED_ID);
            assert!(
                reason.contains("different"),
                "the reason must name the identity difference: {reason}"
            );
        }
        other => panic!("expected StaleCacheEntry, got {other:?}"),
    }

    let repacked = resolve(
        &repacked_manifest,
        RELEASED_ID,
        &default_request(
            &emacs,
            Some(&repacked_input.join("lsp-mode.el")),
            Some(&repacked_input.join("lsp-mode-10.0.0.tar")),
            &root.path().join("cache-fresh"),
        ),
    )?;
    ensure!(
        original.client.version == repacked.client.version
            && original.client.source_sha256 == repacked.client.source_sha256,
        "the two rows share the version string and loaded-file digest"
    );
    ensure!(original.cache_key != repacked.cache_key);
    ensure!(original.client.package_sha256 != repacked.client.package_sha256);
    Ok(())
}

/// The checked manifest pins the implementation-time audited lsp-mode
/// identities (#11746): MELPA Stable 10.0.0 (archive, in-archive file,
/// triple-attested source commit, dependency floor) and the pinned
/// upstream-source tree.
#[test]
fn checked_manifest_pins_the_audited_lsp_mode_subjects() -> Result<()> {
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    manifest.validate().expect("the checked manifest with lsp-mode rows must validate");
    let released = manifest.row_for(RELEASED_ID)?;
    ensure!(
        released.client_kind == SubjectClientKind::LspMode
            && released.source_state == ClientSourceState::Released
            && released.materialization == MaterializationMethod::ExplicitInput
    );
    ensure!(
        released.client_source_sha256
            == "sha256:70466bde62d673a848f7e55d0e2a91d1a11b8fff76bad36ae5ff0c2a59445db0",
        "the released loaded-file digest is the audited in-archive lsp-mode.el digest"
    );
    let package = released
        .external_package
        .as_ref()
        .context("the released row must declare its package identity")?;
    ensure!(
        package.archive_url == "https://stable.melpa.org/packages/lsp-mode-10.0.0.tar"
            && package.archive_sha256
                == "sha256:ad7d46d6bb5b2f840f73c5884cf86dd2678ff6ce68d1bede9ba6b9b60b5668ba",
        "the released row pins the exact MELPA Stable archive bytes"
    );
    ensure!(
        package.attested_source_commit == RELEASED_SOURCE_COMMIT
            && released.emacs_release_tag == RELEASED_SOURCE_COMMIT,
        "the released row's ref is the source commit attested by the MELPA Stable archive, its \
         archive-contents entry, and the GitHub 10.0.0 tag"
    );
    ensure!(package.minimum_emacs == "28.1");
    // Pin the complete audited dependency list, not a count plus one entry:
    // a silently swapped dependency must fail this assertion (review
    // finding on the initial candidate).
    let audited_requires = [
        "emacs 28.1",
        "dash 2.18.0",
        "f 0.21.0",
        "ht 2.3",
        "spinner 1.7.3",
        "markdown-mode 2.3",
        "lv 0",
        "eldoc 1.11",
    ];
    ensure!(
        package.package_requires.iter().map(String::as_str).collect::<Vec<_>>() == audited_requires,
        "the released row must pin the exact audited dependency list in audit order: {:?}",
        package.package_requires
    );
    ensure!(released.source_tree.is_none());

    let source = manifest.row_for(SOURCE_ID)?;
    ensure!(
        source.client_kind == SubjectClientKind::LspMode
            && source.source_state == ClientSourceState::UpstreamSource
            && source.materialization == MaterializationMethod::ExplicitInput
    );
    ensure!(
        source.client_source_sha256
            == "sha256:2878b061e01de2239ae2d1206838cead3a0726c6cb6e7c4280d4248197ab568d",
        "the source loaded-file digest is the audited tree file digest"
    );
    let tree =
        source.source_tree.as_ref().context("the source row must declare its tree identity")?;
    ensure!(
        tree.commit == SOURCE_TREE_COMMIT
            && source.emacs_release_tag == tree.commit
            && tree.tree_sha1 == SOURCE_TREE_SHA1,
        "the source row binds one immutable commit/tree pin"
    );
    ensure!(source.external_package.is_none());
    ensure!(
        source.client_source_sha256 != released.client_source_sha256
            && source.client_version_hint != released.client_version_hint,
        "released and source lsp-mode subjects bind different audited bytes and headers \
         (10.0.0 released vs 10.0.1 source)"
    );
    Ok(())
}

/// The runner registry and the checked manifest agree on the lsp-mode rows,
/// and the lsp-mode subjects dispatch manifest resolution exactly like the
/// Eglot external rows.
#[test]
fn lsp_mode_registry_rows_agree_with_the_manifest() -> Result<()> {
    for id in [RELEASED_ID, SOURCE_ID] {
        let subject = EmacsClientSubject::from_id(id)?;
        ensure!(EmacsClientSubject::known_ids().contains(&id));
        ensure!(subject.resolves_through_subject_manifest());
        let manifest = SubjectManifest::load(&workspace_root()?)?;
        let row = manifest.row_for(id)?;
        ensure!(
            subject.pinned_emacs_version_token() == row.emacs_version_token,
            "registry token and manifest token disagree for {id}"
        );
    }
    ensure!(EmacsClientSubject::from_id(RELEASED_ID)?.requires_client_package());
    ensure!(!EmacsClientSubject::from_id(SOURCE_ID)?.requires_client_package());

    let manifest = SubjectManifest::load(&workspace_root()?)?;
    let registry_external: BTreeSet<String> = EmacsClientSubject::known_ids()
        .iter()
        .filter(|id| {
            EmacsClientSubject::from_id(id).is_ok_and(|subject| {
                subject.resolves_through_subject_manifest()
                    && !subject.resolves_client_source_from_installation()
            })
        })
        .map(|id| id.to_string())
        .collect();
    let manifest_external: BTreeSet<String> = manifest
        .subjects
        .iter()
        .filter(|row| row.client_kind != SubjectClientKind::BundledEglot)
        .map(|row| row.subject_id.clone())
        .collect();
    ensure!(
        registry_external == manifest_external,
        "manifest-bound external registry rows and manifest external rows must cover each other \
         exactly: registry {registry_external:?} vs manifest {manifest_external:?}"
    );
    Ok(())
}

/// The package/Emacs constraint is enforced before any launch-shaped step:
/// the plan builder routes lsp-mode subjects through the manifest resolver,
/// so repacked archive bytes are refused before the not-yet-existing
/// lsp-mode adapter is even reached, and the refusal leaves no output state
/// behind. A positive plan round trip is deliberately absent — no lsp-mode
/// journey is claimed here; the adapter arrives with the lsp-mode lanes.
#[test]
fn run_plan_boundary_enforces_the_archive_constraint_before_launch_machinery() -> Result<()> {
    let root = tempfile::tempdir()?;
    let tree = root.path().join("tree");
    let emacs = fixture_emacs(&tree)?;
    let input = fixture_input_dir(&tree, RELEASED_LSP_MODE_BYTES, Some(REPACKED_ARCHIVE_BYTES))?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = tree.join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    let out_root = tree.join("out");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate,
        client_source: input.join("lsp-mode.el"),
        client_package: Some(input.join("lsp-mode-10.0.0.tar")),
        out_root: out_root.clone(),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root()?,
        EmacsClientSubject::from_id(RELEASED_ID)?,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .err()
    .context("repacked archive bytes must not produce a released lsp-mode plan")?;
    assert!(
        error.to_string().contains("identity mismatch"),
        "the typed rejection must reach the plan boundary before any adapter machinery: {error}"
    );
    ensure!(
        !out_root.exists(),
        "a rejected resolution must leave no run output state behind: {}",
        out_root.display()
    );
    Ok(())
}
