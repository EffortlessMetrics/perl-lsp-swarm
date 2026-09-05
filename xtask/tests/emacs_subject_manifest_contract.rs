//! Discriminating contract tests for the checked immutable Emacs
//! client-subject manifest, resolver, and identity cache (#11744).
//!
//! The first six tests are the issue's first falsifiers, in the order the
//! body lists them:
//!
//! 1. same version, different bundled Eglot file -> reject;
//! 2. Emacs 29 bundled file paired with Emacs 30 subject -> reject;
//! 3. same version, different Emacs executable/build digest -> reject;
//! 4. ambient user package shadows bundled client -> reject;
//! 5. mutable cache entry survives an identity/digest change -> reject;
//! 6. manifest path/hash treated as proof of what runtime loaded -> reject.
//!
//! The remaining tests pin the positive contract: the checked manifest rows
//! (with the implementation-time GNU-tarball audit digests), registry
//! coherence, typed unknown/unpinned/ambient/unavailable dispositions,
//! floating-ref rejection, deterministic cache keys over the complete
//! subject identity, and the round-trip through the real shared run-plan
//! boundary.

// Plain #[test] functions assert through the standard panic-on-failure
// idiom; these tests are proof, not production paths. `expect` (not
// `allow`) so Clippy flags the suppression once the idiom moves on.
#![expect(clippy::expect_used, clippy::panic)]

use anyhow::{Context, Result, ensure};
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_host_run::{self, EmacsClientSubject, build_client_subject_run_plan};
use xtask::emacs_subject_manifest::{
    CACHE_ENTRY_FILE, MANIFEST_RELATIVE_PATH, ResolveFailure, ResolveRequest, SubjectClientKind,
    SubjectManifest, SubjectRejection, SubjectRow, resolve,
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

/// Fixture bytes standing in for the two audited upstream `eglot.el` files.
/// The resolver treats them as opaque digest-bound content; the checked
/// manifest's real rows carry the real audited digests and are pinned by
/// their own tests below.
const EMACS_29_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- Emacs 29.4 bundled client (fixture)\n;; Version: 1.12.29\n";
const EMACS_30_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- Emacs 30.1 bundled client (fixture)\n;; Version: 1.17.30\n";

fn bundled_row(
    subject_id: &str,
    release_tag: &str,
    version_token: &str,
    version_hint: &str,
    client_source_sha256: String,
) -> SubjectRow {
    SubjectRow {
        subject_id: subject_id.to_string(),
        client_kind: SubjectClientKind::BundledEglot,
        source_state: ClientSourceState::Bundled,
        emacs_release_tag: release_tag.to_string(),
        emacs_version_token: version_token.to_string(),
        client_version_hint: version_hint.to_string(),
        client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
        client_source_sha256,
        materialization:
            xtask::emacs_subject_manifest::MaterializationMethod::InstallationRootResolution,
        client_library_forms: vec![
            "eglot.el".to_string(),
            "eglot.elc".to_string(),
            "eglot.el.gz".to_string(),
        ],
        external_package: None,
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://ftp.gnu.org/gnu/emacs/fixture.tar.xz".to_string(),
            gnu_tarball_sha256: sha256_of(b"fixture tarball"),
            observed_client_version_header: version_hint.to_string(),
        },
    }
}

fn fixture_manifest() -> SubjectManifest {
    SubjectManifest {
        schema_version: xtask::emacs_subject_manifest::MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![
            bundled_row(
                "bundled_eglot_emacs_29_4",
                "emacs-29.4",
                "29.4",
                "1.12.29",
                sha256_of(EMACS_29_EGLOT_BYTES),
            ),
            bundled_row(
                "bundled_eglot_emacs_30_1",
                "emacs-30.1",
                "30.1",
                "1.17.30",
                sha256_of(EMACS_30_EGLOT_BYTES),
            ),
        ],
    }
}

/// Materialize a fake exact Emacs installation: `<root>/bin/emacs` plus the
/// bundled client file under the real in-tree location
/// `<root>/share/emacs/<version>/lisp/progmodes/`.
fn fixture_installation(
    root: &Path,
    emacs_version: &str,
    client_bytes: &[u8],
    client_file_name: &str,
    executable_bytes: &[u8],
) -> Result<PathBuf> {
    let bin = root.join("bin");
    let progmodes = root.join("share/emacs").join(emacs_version).join("lisp/progmodes");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&progmodes)?;
    let emacs = bin.join("emacs");
    fs::write(&emacs, executable_bytes)?;
    fs::write(progmodes.join(client_file_name), client_bytes)?;
    Ok(emacs)
}

fn default_request<'a>(emacs_executable: &'a Path, cache_root: &'a Path) -> ResolveRequest<'a> {
    ResolveRequest {
        emacs_executable,
        client_source: None,
        client_package: None,
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

/// Falsifier 1: a file that still claims the pinned version header but has
/// different bytes is a different subject, not this one.
#[test]
fn same_version_different_bundled_eglot_file_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let same_version_different_bytes =
        b";; eglot.el --- tampered build\n;; Version: 1.17.30\n;; different bytes\n";
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        same_version_different_bytes,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let failure = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect_err("a different bundled file with the same version must be rejected");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains(&sha256_of(same_version_different_bytes)),
                "the mismatch reason must name the observed digest: {reason}"
            );
            assert!(
                reason.contains(&sha256_of(EMACS_30_EGLOT_BYTES)),
                "the mismatch reason must name the pinned digest: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    assert!(!cache.exists(), "a rejected resolution must not write cache state");
}

/// Falsifier 2: pairing the Emacs 29.4 generation file with the Emacs 30.1
/// subject is an identity mismatch, while the same file satisfies the 29.4
/// subject (the pairing is wrong, not the file).
#[test]
fn emacs_29_bundled_file_with_emacs_30_subject_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "29.4",
        EMACS_29_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 29.4 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs, &root.path().join("cache")),
    )
    .expect_err("the Emacs 29 bundled file cannot satisfy the Emacs 30 subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("emacs-30.1") && reason.contains("emacs-29.4"),
                "the reason must name the intended and observed generations: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    // The same installation is exactly the 29.4 subject.
    let resolved = resolve(
        &manifest,
        "bundled_eglot_emacs_29_4",
        &ResolveRequest {
            emacs_executable: &emacs,
            client_source: None,
            client_package: None,
            cache_root: &root.path().join("cache-29"),
            probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
        },
    )
    .expect("the 29.4 generation file satisfies the 29.4 subject");
    assert_eq!(resolved.client.version, "1.12.29");
    assert_eq!(resolved.client.source_ref, "emacs-29.4");
}

/// Falsifier 3: the same visible version with a different executable/build
/// digest is a different subject instance; a cache holding one build must
/// not satisfy the other.
#[test]
fn same_version_different_emacs_build_digest_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    // Each installation gets its own isolated root so one build's
    // resolution walk can never see the other build's library.
    let emacs_a = fixture_installation(
        &root.path().join("build-a"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"build A of GNU Emacs 30.1",
    )
    .expect("installation A");
    let emacs_b = fixture_installation(
        &root.path().join("build-b"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"build B of GNU Emacs 30.1",
    )
    .expect("installation B");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let first = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs_a, &cache))
        .expect("build A resolves against a fresh cache");
    assert!(!first.reused_cache);
    let failure =
        resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs_b, &cache))
            .expect_err("build B must not be satisfied by build A's cache identity");
    match rejection_of(&failure) {
        SubjectRejection::StaleCacheEntry { subject_id, reason, .. } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("different Emacs build digest"),
                "the reason must name the build-digest difference: {reason}"
            );
        }
        other => panic!("expected StaleCacheEntry, got {other:?}"),
    }
    // A different build is legitimate under its own bounded cache location.
    let second = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs_b, &root.path().join("cache-b")),
    )
    .expect("build B resolves against its own fresh cache");
    assert_ne!(first.cache_key, second.cache_key);
}

/// Falsifier 4a: an ambient ELPA-layout copy of the client — even one whose
/// bytes match the pinned digest — cannot satisfy a bundled subject.
#[test]
fn ambient_user_package_cannot_shadow_the_bundled_client() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        &root.path().join("install"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    // A user ELPA package directory in the ambient home, outside the exact
    // installation, carrying a byte-identical copy of the client.
    let elpa_copy = root.path().join("home/.emacs.d/elpa/eglot-1.17.30/eglot.el");
    fs::create_dir_all(elpa_copy.parent().expect("parent")).expect("elpa dir");
    fs::write(&elpa_copy, EMACS_30_EGLOT_BYTES).expect("elpa copy");
    let manifest = fixture_manifest();
    let request = ResolveRequest {
        emacs_executable: &emacs,
        client_source: Some(&elpa_copy),
        client_package: None,
        cache_root: &root.path().join("cache"),
        probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
    };
    let failure = resolve(&manifest, "bundled_eglot_emacs_30_1", &request)
        .expect_err("an ambient ELPA copy must not satisfy a bundled subject");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { subject_id, reason } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("outside the exact Emacs installation"),
                "the reason must name the containment boundary: {reason}"
            );
        }
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }
}

/// Falsifier 4b: a package-layout copy inside the installation tree
/// (site-lisp) is ambient package state, not the bundled client.
#[test]
fn site_lisp_package_layout_inside_the_installation_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation with the true in-tree copy");
    // A second same-form copy under a site-lisp layout would be found by the
    // plain walk; remove the in-tree copy so resolution can only see the
    // site-lisp one.
    fs::remove_file(root.path().join("share/emacs/30.1/lisp/progmodes/eglot.el"))
        .expect("remove in-tree copy");
    let site_lisp = root.path().join("share/emacs/site-lisp/eglot.el");
    fs::create_dir_all(site_lisp.parent().expect("parent")).expect("site-lisp dir");
    fs::write(&site_lisp, EMACS_30_EGLOT_BYTES).expect("site-lisp copy");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs, &root.path().join("cache")),
    )
    .expect_err("a site-lisp package layout is ambient state");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { reason, .. } => assert!(
            reason.contains("package layout"),
            "the reason must name the package layout: {reason}"
        ),
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }
}

/// Falsifier 4c: the immutable cache entry must contain exactly its record;
/// unexpected package-like files inside it are ambient state.
#[test]
fn cache_entry_rejects_unexpected_files() {
    let root = tempfile::tempdir().expect("tempdir");
    // The bounded cache lives outside the installation walk root.
    let emacs = fixture_installation(
        &root.path().join("install"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect("first resolution materializes the entry");
    let entry = &resolved.cache_entry;
    assert!(
        entry.join(CACHE_ENTRY_FILE).is_file(),
        "the entry record must exist after materialization"
    );
    fs::create_dir_all(entry.join("elpa/eglot-1.17.30")).expect("ambient dir");
    fs::write(entry.join("elpa/eglot-1.17.30/eglot.el"), EMACS_30_EGLOT_BYTES)
        .expect("ambient copy");
    let failure = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect_err("an entry carrying package files is ambient state");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { reason, .. } => assert!(
            reason.contains("unexpected files"),
            "the reason must name the unexpected files: {reason}"
        ),
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }
}

/// Falsifier 5: when the manifest identity is revised (declared digest
/// change), an existing cache entry must not survive as a satisfier of the
/// changed subject.
#[test]
fn mutable_cache_entry_cannot_satisfy_a_changed_identity() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let first = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect("initial resolution");
    let record_before = fs::read(first.cache_entry.join(CACHE_ENTRY_FILE)).expect("record");

    // The row is revised: a new declared digest (and the installation file
    // is revised to match), like an upstream re-pin.
    let revised_bytes = b";; eglot.el --- revised pin\n;; Version: 1.17.31\n";
    let progmodes = root.path().join("share/emacs/30.1/lisp/progmodes/eglot.el");
    fs::write(&progmodes, revised_bytes).expect("revised file");
    let mut revised = fixture_manifest();
    revised.subjects[1].client_source_sha256 = sha256_of(revised_bytes);
    revised.subjects[1].client_version_hint = "1.17.31".to_string();
    revised.subjects[1].digest_audit.observed_client_version_header = "1.17.31".to_string();

    let failure = resolve(&revised, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect_err("a changed subject identity must not be satisfied by the old entry");
    match rejection_of(&failure) {
        SubjectRejection::StaleCacheEntry { subject_id, reason, .. } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("fresh bounded cache location"),
                "the rejection must require a fresh bounded cache location: {reason}"
            );
        }
        other => panic!("expected StaleCacheEntry, got {other:?}"),
    }
    let record_after = fs::read(first.cache_entry.join(CACHE_ENTRY_FILE)).expect("record");
    assert!(record_before == record_after, "the immutable entry must not be mutated in place");
}

/// Falsifier 6: manifest identity is intended input. A cache record that
/// tries to carry a runtime-load claim is rejected, and the record's
/// boundary field states the intended-input limit explicitly.
#[test]
fn manifest_identity_is_never_treated_as_runtime_proof() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect("resolution");
    let record_path = resolved.cache_entry.join(CACHE_ENTRY_FILE);
    let record = fs::read_to_string(&record_path).expect("record");
    assert!(
        record.contains("intended input"),
        "the record must carry the intended-input boundary: {record}"
    );
    assert!(
        !record.contains("loaded") && !record.contains("runtime_pass"),
        "the record vocabulary must not claim any runtime load: {record}"
    );

    // A doctored record that claims what the runtime loaded cannot even be
    // parsed back: the cache schema has no such field.
    let mut doctored: serde_json::Value = serde_json::from_str(&record).expect("json");
    let object = doctored.as_object_mut().expect("object");
    object.insert("runtime_loaded_sha256".to_string(), serde_json::json!("sha256:deadbeef"));
    fs::write(&record_path, serde_json::to_vec_pretty(&doctored).expect("serialize"))
        .expect("doctored record");
    let failure = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))
        .expect_err("a doctored record claiming runtime proof must fail closed");
    match &failure {
        ResolveFailure::Instrument(message) => assert!(
            message.contains("runtime_loaded_sha256") || message.contains("unknown field"),
            "the failure must name the rejected runtime-proof claim: {message}"
        ),
        ResolveFailure::Rejected(rejection) => {
            panic!("expected an instrument failure on the corrupt record, got {rejection:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// Positive contract and remaining dispositions
// ---------------------------------------------------------------------------

/// The checked manifest loads, validates, and pins exactly the two audited
/// bundled Eglot subjects from the implementation-time re-audit (GNU
/// tarball emacs-29.4 / emacs-30.1, cross-checked against the Savannah and
/// GitHub-mirror release tags).
#[test]
fn checked_manifest_pins_the_audited_bundled_subjects() -> Result<()> {
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    let row_29 = manifest.row_for("bundled_eglot_emacs_29_4")?;
    let row_30 = manifest.row_for("bundled_eglot_emacs_30_1")?;
    assert_eq!(
        row_29.client_source_sha256,
        "sha256:1d94e789d4d08119b5ed631468627c7f052cc268d084ccf881a3d317abc1ab2c"
    );
    assert_eq!(
        row_30.client_source_sha256,
        "sha256:f20303322b1ddb3231a133dd7543ec3b9ba86f01e92d608c859b5ecad3cd2d7b"
    );
    assert_eq!(row_29.emacs_release_tag, "emacs-29.4");
    assert_eq!(row_30.emacs_release_tag, "emacs-30.1");
    assert_eq!(row_29.client_version_hint, "1.12.29");
    assert_eq!(row_30.client_version_hint, "1.17.30");
    assert_eq!(row_29.subject_id, row_29.subject_id.to_lowercase());
    ensure!(
        manifest.subjects.iter().any(|row| row.client_kind == SubjectClientKind::BundledEglot),
        "the manifest must keep carrying the bundled rows; the external Eglot rows are pinned by \
         their own #11745 contract tests"
    );
    Ok(())
}

/// The manifest's bundled rows cover exactly the runner registry's
/// installation-resolution rows: no drift in either direction, and the
/// registry's pinned version tokens equal the manifest rows' tokens.
/// External rows (#11745/#11746) are pinned by their own contract tests.
#[test]
fn checked_manifest_covers_exactly_the_bundled_registry_rows() -> Result<()> {
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    let registry_bundled: BTreeSet<String> = EmacsClientSubject::known_ids()
        .iter()
        .filter(|id| {
            EmacsClientSubject::from_id(id)
                .is_ok_and(|subject| subject.resolves_client_source_from_installation())
        })
        .map(|id| id.to_string())
        .collect();
    let manifest_bundled: BTreeSet<String> = manifest
        .subjects
        .iter()
        .filter(|row| row.client_kind == SubjectClientKind::BundledEglot)
        .map(|row| row.subject_id.clone())
        .collect();
    ensure!(
        registry_bundled == manifest_bundled,
        "bundled registry rows and manifest rows must cover each other exactly: registry \
         {registry_bundled:?} vs manifest {manifest_bundled:?}"
    );
    for id in &registry_bundled {
        let subject = EmacsClientSubject::from_id(id)?;
        let row = manifest.row_for(id)?;
        ensure!(
            subject.pinned_emacs_version_token() == row.emacs_version_token,
            "registry token and manifest token disagree for {id}"
        );
    }
    Ok(())
}

/// The resolver binds the exact pinned subject: identity fields all come
/// from the row, the client digest equals the declared digest, the
/// executable digest is bound at resolution, and repeat resolution reuses
/// the immutable entry deterministically.
#[test]
fn resolver_binds_the_exact_pinned_subject() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )?;
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved =
        resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))?;
    ensure!(!resolved.reused_cache, "a fresh cache must materialize");
    ensure!(resolved.client.client_id == "bundled_eglot_emacs_30_1");
    ensure!(
        resolved.client.kind == emacs_host_run::emacs_host_runner::EmacsClientKind::BundledEglot
    );
    ensure!(resolved.client.version == "1.17.30");
    ensure!(resolved.client.source_state == ClientSourceState::Bundled);
    ensure!(resolved.client.source_ref == "emacs-30.1");
    ensure!(resolved.client.source_sha256 == sha256_of(EMACS_30_EGLOT_BYTES));
    ensure!(resolved.client.package_sha256.is_none());
    ensure!(
        resolved.emacs_build_sha256 == sha256_of(b"fake exact emacs 30.1 executable"),
        "the executable digest is part of the bound identity"
    );
    let again = resolve(&manifest, "bundled_eglot_emacs_30_1", &default_request(&emacs, &cache))?;
    ensure!(again.reused_cache, "the same identity must reuse the entry");
    ensure!(again.cache_key == resolved.cache_key);
    Ok(())
}

/// Unknown or unpinned subject ids are typed errors listing the registry,
/// never a loose match.
#[test]
fn unknown_subject_ids_are_typed_errors_listing_the_registry() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    for unknown in ["eglot", "bundled_eglot_emacs_31_1", "bundled_eglot"] {
        let failure =
            resolve(&manifest, unknown, &default_request(&emacs, &root.path().join("cache")))
                .expect_err("unknown ids must be typed errors");
        match rejection_of(&failure) {
            SubjectRejection::UnknownSubject { requested, known_subjects } => {
                assert_eq!(requested, unknown);
                assert!(
                    known_subjects.contains(&"bundled_eglot_emacs_29_4".to_string())
                        && known_subjects.contains(&"bundled_eglot_emacs_30_1".to_string()),
                    "the error must list the checked registry: {known_subjects:?}"
                );
            }
            other => panic!("expected UnknownSubject, got {other:?}"),
        }
    }
}

/// Floating refs and mutable aliases are rejected at manifest validation.
#[test]
fn floating_refs_and_mutable_aliases_are_rejected() {
    for floating in ["main", "HEAD", "trunk", "latest", "emacs-30", "release"] {
        let mut manifest = fixture_manifest();
        manifest.subjects[1].emacs_release_tag = floating.to_string();
        let rejection = manifest.validate().expect_err("a floating ref must fail validation");
        match &rejection {
            SubjectRejection::InvalidManifest { reason } => assert!(
                reason.contains("floating"),
                "the reason must name the floating-ref rule: {reason}"
            ),
            other => panic!("expected InvalidManifest, got {other:?}"),
        }
    }
    // An exact commit ref is not floating.
    let mut manifest = fixture_manifest();
    manifest.subjects[1].emacs_release_tag = "0f4aa7b3d4b3b7f2f83a2b7a694af4a3d2c1b0a9".to_string();
    manifest.subjects[1].emacs_version_token = "30.1".to_string();
    // A commit tag has no numeric token suffix, so the token-coherence rule
    // is checked only for emacs-x.y tags; commit pins stay exact.
    manifest
        .validate()
        .unwrap_or_else(|rejection| panic!("an exact commit pin is not floating: {rejection:?}"));
}

/// A gzip-wrapped bundled library validates through its decompressed
/// content digest.
#[test]
fn gzipped_bundled_library_validates_through_decompressed_digest() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(EMACS_30_EGLOT_BYTES)?;
    let gzipped = encoder.finish()?;
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        &gzipped,
        "eglot.el.gz",
        b"fake exact emacs 30.1 executable",
    )?;
    let manifest = fixture_manifest();
    let resolved = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs, &root.path().join("cache")),
    )?;
    ensure!(
        resolved.client.source_sha256 == sha256_of(EMACS_30_EGLOT_BYTES),
        "the gz form binds the decompressed source digest"
    );
    ensure!(
        resolved.cache_entry.to_string_lossy().ends_with(&resolved.cache_key),
        "the entry lives under the complete identity key"
    );
    Ok(())
}

/// A compiled-only installation cannot validate the declared upstream
/// source digest and is an explicit unavailable disposition, never a
/// fallback load.
#[test]
fn compiled_only_installation_is_typed_unavailable() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        b"bytecode that is not the declared source",
        "eglot.elc",
        b"fake exact emacs 30.1 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs, &root.path().join("cache")),
    )
    .expect_err("a compiled-only installation is unavailable for this subject");
    match rejection_of(&failure) {
        SubjectRejection::UnavailableSubject { subject_id, reason } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("compiled-only"),
                "the reason must name the compiled-only boundary: {reason}"
            );
        }
        other => panic!("expected UnavailableSubject, got {other:?}"),
    }
}

/// A probed host version line without the pinned token is an explicit
/// incompatibility, never a silent mismatch.
#[test]
fn probed_version_token_mismatch_is_incompatible() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 31.0.50 executable",
    )
    .expect("installation");
    let manifest = fixture_manifest();
    let request = ResolveRequest {
        emacs_executable: &emacs,
        client_source: None,
        client_package: None,
        cache_root: &root.path().join("cache"),
        probed_emacs_version: Some("GNU Emacs 31.0.50 (development)"),
    };
    let failure = resolve(&manifest, "bundled_eglot_emacs_30_1", &request)
        .expect_err("a host without the pinned token is a different subject");
    match rejection_of(&failure) {
        SubjectRejection::IncompatibleSubject { subject_id, reason } => {
            assert_eq!(subject_id, "bundled_eglot_emacs_30_1");
            assert!(
                reason.contains("31.0.50") && reason.contains("30.1"),
                "the reason must name observed and pinned tokens: {reason}"
            );
        }
        other => panic!("expected IncompatibleSubject, got {other:?}"),
    }
}

/// Cache keys are derived from the complete subject identity — the same
/// version hint under different release tags, different declared digests,
/// different executable digests, and different library forms each produce
/// different keys; identical identities produce identical keys.
#[test]
fn cache_keys_bind_the_complete_identity_not_the_version_alone() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_installation(
        &root.path().join("base"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )?;
    let manifest = fixture_manifest();
    let base = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs, &root.path().join("cache-a")),
    )?;

    let emacs_29 = fixture_installation(
        &root.path().join("tree-29"),
        "29.4",
        EMACS_29_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 29.4 executable",
    )?;
    let other_generation = resolve(
        &manifest,
        "bundled_eglot_emacs_29_4",
        &ResolveRequest {
            emacs_executable: &emacs_29,
            client_source: None,
            client_package: None,
            cache_root: &root.path().join("cache-b"),
            probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
        },
    )?;
    ensure!(
        base.cache_key != other_generation.cache_key,
        "different generations must never share a cache key"
    );

    let gzipped_root = tempfile::tempdir()?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(EMACS_30_EGLOT_BYTES)?;
    let gzipped = encoder.finish()?;
    let emacs_gz = fixture_installation(
        gzipped_root.path(),
        "30.1",
        &gzipped,
        "eglot.el.gz",
        b"fake exact emacs 30.1 executable",
    )?;
    let gz_form = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs_gz, &gzipped_root.path().join("cache")),
    )?;
    ensure!(
        base.cache_key != gz_form.cache_key,
        "the resolved library form is part of the identity"
    );

    let emacs_rebuild = fixture_installation(
        &root.path().join("tree-rebuild"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"rebuilt fake exact emacs 30.1 executable",
    )?;
    let rebuild = resolve(
        &manifest,
        "bundled_eglot_emacs_30_1",
        &default_request(&emacs_rebuild, &root.path().join("cache-c")),
    )?;
    ensure!(
        base.cache_key != rebuild.cache_key,
        "a different build digest is a different identity"
    );
    Ok(())
}

/// A missing manifest file fails closed as an instrument error, never as an
/// empty registry.
#[test]
fn missing_manifest_file_fails_closed() {
    let empty = tempfile::tempdir().expect("tempdir");
    let failure = SubjectManifest::load(&empty.path().join("nowhere"))
        .expect_err("a missing manifest must not load");
    assert!(
        failure.to_string().contains(MANIFEST_RELATIVE_PATH),
        "the failure must name the checked manifest path: {failure}"
    );
}

/// Both bundled subjects round-trip through the real shared run-plan
/// boundary: the resolver's run inputs feed the landed plan builder, and
/// the plan validates over digest-verified files.
#[test]
fn bundled_subjects_round_trip_through_the_run_plan_boundary() -> Result<()> {
    let root = tempfile::tempdir()?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    for (subject_id, version_line) in [
        ("bundled_eglot_emacs_29_4", "GNU Emacs 29.4 (fixture)"),
        ("bundled_eglot_emacs_30_1", "GNU Emacs 30.1 (fixture)"),
    ] {
        let subject = EmacsClientSubject::from_id(subject_id)?;
        let tree = root.path().join(subject_id);
        let (client_bytes, emacs_version_dir) = if subject_id == "bundled_eglot_emacs_29_4" {
            (EMACS_29_EGLOT_BYTES, "29.4")
        } else {
            (EMACS_30_EGLOT_BYTES, "30.1")
        };
        let emacs = fixture_installation(
            &tree,
            emacs_version_dir,
            client_bytes,
            "eglot.el",
            b"fake exact emacs executable",
        )?;
        let candidate = tree.join(candidate_name);
        fs::write(&candidate, b"fake exact perllsp candidate bytes")?;

        // Resolver round-trip on the fixture manifest (declared digests
        // match the fixture bytes).
        let mut manifest = fixture_manifest();
        manifest.subjects.retain(|row| row.subject_id == subject_id);
        let cache = tree.join("cache");
        let resolved = resolve(
            &manifest,
            subject_id,
            &ResolveRequest {
                emacs_executable: &emacs,
                client_source: None,
                client_package: None,
                cache_root: &cache,
                probed_emacs_version: Some(version_line),
            },
        )?;
        ensure!(
            resolved.client.source_sha256 == sha256_of(client_bytes),
            "the resolved client digest must equal the declared digest"
        );

        // Run-plan boundary round-trip: the resolved inputs are exactly the
        // landed builder's inputs, over the checked tree, and the builder
        // re-validates the declared digest through the resolver before the
        // plan exists.
        let run = resolved.host_run_inputs(&candidate, &tree.join("out"), 0);
        let (plan, _layout) = build_client_subject_run_plan(
            &workspace_root()?,
            subject,
            &run,
            &"0".repeat(40),
            "perllsp fake",
            version_line,
            &manifest,
        )?;
        plan.validate()?;
        ensure!(
            plan.identity.client.client_id == subject_id,
            "the plan binds the resolved subject id"
        );
        ensure!(
            plan.identity.emacs_build_sha256 == resolved.emacs_build_sha256,
            "the plan binds the same executable digest the resolver bound"
        );
    }
    Ok(())
}

/// The runner registry now carries both bundled generations, and the
/// checked manifest is the single identity authority for them.
#[test]
fn registry_and_manifest_agree_on_both_bundled_generations() -> Result<()> {
    ensure!(
        EmacsClientSubject::known_ids().contains(&"bundled_eglot_emacs_29_4"),
        "the Emacs 29.4 bundled generation is a registry row"
    );
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    let subject = EmacsClientSubject::from_id("bundled_eglot_emacs_29_4")?;
    let row = manifest.row_for("bundled_eglot_emacs_29_4")?;
    let identity =
        subject.client_identity(&manifest, format!("sha256:{}", "3".repeat(64)), None)?;
    ensure!(identity.client_id == row.subject_id);
    ensure!(identity.version == row.client_version_hint);
    ensure!(identity.source_ref == row.emacs_release_tag);
    ensure!(identity.source_state == row.source_state);
    Ok(())
}

/// The run-plan boundary itself refuses unchecked client bytes: a file
/// whose digest binds no declared row cannot produce a plan labeled as the
/// checked subject, and the refusal leaves no output state behind. Identity
/// strings never travel ahead of digest validation (#11744 review
/// finding).
#[test]
fn run_plan_boundary_rejects_client_bytes_that_bind_no_declared_digest() -> Result<()> {
    let root = tempfile::tempdir()?;
    // The installation carries the 29.4-generation bytes while the plan
    // asks for the 30.1 subject of the same fixture manifest.
    let emacs = fixture_installation(
        root.path(),
        "30.1",
        EMACS_29_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = root.path().join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    let client_source = root.path().join("share/emacs/30.1/lisp/progmodes/eglot.el");
    let out_root = root.path().join("out");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate,
        client_source,
        client_package: None,
        out_root: out_root.clone(),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root()?,
        EmacsClientSubject::BundledEglotEmacs301,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .err()
    .context("cross-generation client bytes must not produce a 30.1 plan")?;
    assert!(
        error.to_string().contains("identity mismatch"),
        "the typed rejection must reach the plan boundary: {error}"
    );
    assert!(
        error.to_string().contains("bundled_eglot_emacs_30_1"),
        "the failure must name the requested subject: {error}"
    );
    ensure!(
        !out_root.exists(),
        "a rejected resolution must leave no run output state behind: {}",
        out_root.display()
    );
    Ok(())
}

/// Ambient package state cannot reach the run-plan boundary either: an
/// explicit client file outside the exact Emacs installation is rejected
/// before a plan exists, even when its bytes match the declared digest.
#[test]
fn run_plan_boundary_rejects_ambient_client_state() -> Result<()> {
    let root = tempfile::tempdir()?;
    let install = root.path().join("install");
    let emacs = fixture_installation(
        &install,
        "30.1",
        EMACS_30_EGLOT_BYTES,
        "eglot.el",
        b"fake exact emacs 30.1 executable",
    )?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = root.path().join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    // A byte-identical ambient ELPA copy outside the installation.
    let ambient = root.path().join("home/.emacs.d/elpa/eglot-1.17.30/eglot.el");
    fs::create_dir_all(ambient.parent().context("elpa parent")?)?;
    fs::write(&ambient, EMACS_30_EGLOT_BYTES)?;
    let out_root = root.path().join("out");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate,
        client_source: ambient,
        client_package: None,
        out_root: out_root.clone(),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root()?,
        EmacsClientSubject::BundledEglotEmacs301,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .err()
    .context("an ambient ELPA copy must not produce a plan")?;
    assert!(
        error.to_string().contains("ambient state"),
        "the typed ambient rejection must reach the plan boundary: {error}"
    );
    ensure!(!out_root.exists(), "no run output state may exist after the refusal");
    Ok(())
}
