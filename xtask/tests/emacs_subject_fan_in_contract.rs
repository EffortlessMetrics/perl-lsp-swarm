//! Fan-in contract tests for the complete Emacs subject denominator (#8755,
//! SUBJ_FAN over the #11744/#11745/#11746 subject lane).
//!
//! The fan-in node's controls, in train-manifest order:
//!
//! 1. positive — the complete subject denominator validates over the real
//!    checked manifest (`complete_denominator_validates_over_the_checked_manifest`);
//! 2. opposite — a partial denominator is never rendered complete, slot by
//!    slot (`partial_denominators_are_refused_slot_by_slot`);
//! 3. wrong subject — a manifest row citing an unbound generation is
//!    refused (`unbound_generation_rows_are_refused`), and a duplicate row
//!    under any id cannot ride along behind the first match
//!    (`duplicate_rows_under_a_bound_id_are_refused`, a PR review repair);
//! 4. stale — a bound id whose row binds a different generation is refused
//!    (`stale_generation_under_a_bound_id_is_refused`);
//! 5. fault — substituted material is a typed rejection through the joint
//!    resolution, never a pass
//!    (`substituted_material_is_typed_rejected_across_families_and_generations`);
//! 6. mutation — the fan-in executes no missing subject work: validation is
//!    pure over declared identity and leaves a partial manifest partial,
//!    while the coherent six-subject proof binds material through the
//!    landed resolver without launching anything
//!    (`all_denominator_subjects_resolve_in_one_coherent_proof`,
//!    `launch_refusals_are_preserved_across_the_denominator`).
//!
//! The six-subject fixtures mirror the real checked rows' identity shapes
//! (ids, kinds, states, tags, tokens, headers, commit/tree pins) with
//! fixture digests; the real audited digests stay pinned by the per-family
//! contract suites (#12540/#12662/#12669). No journey, capability,
//! observation, or root claim is made here: the subjects that materialize
//! without a driver adapter keep their typed launch refusals.

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
use xtask::emacs_host_run::{self, EmacsClientSubject};
use xtask::emacs_subject_fan_in::{
    SUBJECT_DENOMINATOR, SubjectFanInFailure, validate_subject_lane_denominator,
};
use xtask::emacs_subject_manifest::{
    CACHE_ENTRY_FILE, ExternalPackageIdentity, MaterializationMethod, ResolveFailure,
    ResolveRequest, SourceTreeIdentity, SubjectClientKind, SubjectInputRecord, SubjectManifest,
    SubjectRejection, SubjectRow, resolve,
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

// ---------------------------------------------------------------------------
// Fixture material: the six denominator classes with fixture bytes
// ---------------------------------------------------------------------------

/// Fixture stand-ins for the audited client files. The released/source
/// Eglot pair deliberately shares the `1.24` header with different bytes
/// (the audited resemblance), and the lsp-mode pair differs in both header
/// and bytes (10.0.0 released vs 10.0.1 source).
const EMACS_29_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- Emacs 29.4 bundled client (fan-in fixture)\n;; Version: 1.12.29\n";
const EMACS_30_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- Emacs 30.1 bundled client (fan-in fixture)\n;; Version: 1.17.30\n";
const RELEASED_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- GNU ELPA 1.24 released client (fan-in fixture)\n;; Version: 1.24\n";
const SOURCE_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- emacs.git c1ad9d27 source client (fan-in fixture)\n;; Version: 1.24\n";
const RELEASED_LSP_MODE_BYTES: &[u8] =
    b";; lsp-mode.el --- MELPA Stable 10.0.0 released client (fan-in fixture)\n;; Version: 10.0.0\n";
const SOURCE_LSP_MODE_BYTES: &[u8] =
    b";; lsp-mode.el --- emacs-lsp 6bfc593 source client (fan-in fixture)\n;; Version: 10.0.1\n";

const RELEASED_EGLOT_ARCHIVE: &[u8] = b"fan-in fixture GNU ELPA eglot-1.24.tar bytes";
const RELEASED_LSP_MODE_ARCHIVE: &[u8] = b"fan-in fixture MELPA Stable lsp-mode-10.0.0.tar bytes";

const RELEASED_EGLOT_COMMIT: &str = "0d67e76b94e1f0af9fe364aed8aa5db1c494c206";
const SOURCE_EGLOT_COMMIT: &str = "c1ad9d27207aff96a22d49ae4c6cab35a2619927";
const SOURCE_EGLOT_TREE: &str = "dc5475f03a6462846d36ade5a68a2e90a2578087";
const RELEASED_LSP_MODE_COMMIT: &str = "913a6c07f163205cb568bc68d7dfe677dbc358ab";
const SOURCE_LSP_MODE_COMMIT: &str = "6bfc593d7b1bc0dd656f09ffce52cc085ebced05";
const SOURCE_LSP_MODE_TREE: &str = "b9111a657fe1376f92d203ba4951868fb0fa3f57";

const BUNDLED_29_ID: &str = "bundled_eglot_emacs_29_4";
const BUNDLED_30_ID: &str = "bundled_eglot_emacs_30_1";
const RELEASED_EGLOT_ID: &str = "released_eglot_gnu_elpa_1_24";
const SOURCE_EGLOT_ID: &str = "source_eglot_emacs_c1ad9d27";
const RELEASED_LSP_MODE_ID: &str = "released_lsp_mode_melpa_stable_10_0_0";
const SOURCE_LSP_MODE_ID: &str = "source_lsp_mode_github_6bfc593";

fn bundled_row(subject_id: &str, tag: &str, token: &str, hint: &str, digest: String) -> SubjectRow {
    SubjectRow {
        subject_id: subject_id.to_string(),
        client_kind: SubjectClientKind::BundledEglot,
        source_state: ClientSourceState::Bundled,
        emacs_release_tag: tag.to_string(),
        emacs_version_token: token.to_string(),
        client_version_hint: hint.to_string(),
        client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
        client_source_sha256: digest,
        materialization: MaterializationMethod::InstallationRootResolution,
        client_library_forms: vec![
            "eglot.el".to_string(),
            "eglot.elc".to_string(),
            "eglot.el.gz".to_string(),
        ],
        external_package: None,
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://ftp.gnu.org/gnu/emacs/fan-in-fixture.tar.xz".to_string(),
            gnu_tarball_sha256: sha256_of(b"fan-in fixture GNU tarball"),
            observed_client_version_header: hint.to_string(),
        },
    }
}

/// The released Eglot 1.24 class with fixture digests: every identity
/// field except the digests mirrors the checked row.
fn released_eglot_row(client_digest: String, archive_digest: String) -> SubjectRow {
    SubjectRow {
        subject_id: RELEASED_EGLOT_ID.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::Released,
        emacs_release_tag: RELEASED_EGLOT_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "eglot.el".to_string(),
        client_source_sha256: client_digest,
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["eglot.el".to_string()],
        external_package: Some(ExternalPackageIdentity {
            archive_url: "https://elpa.gnu.org/packages/fan-in-eglot-fixture.tar".to_string(),
            archive_sha256: archive_digest.clone(),
            attested_source_commit: RELEASED_EGLOT_COMMIT.to_string(),
            package_requires: vec!["emacs 26.3".to_string(), "fixture-dep 1.0".to_string()],
            minimum_emacs: "26.3".to_string(),
            checksum_disposition: "fan_in_fixture_archive_sha256".to_string(),
        }),
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://elpa.gnu.org/packages/fan-in-eglot-fixture.tar".to_string(),
            gnu_tarball_sha256: archive_digest,
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

/// The pinned upstream-source Eglot class with a fixture digest.
fn source_eglot_row(client_digest: String) -> SubjectRow {
    SubjectRow {
        subject_id: SOURCE_EGLOT_ID.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: SOURCE_EGLOT_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
        client_source_sha256: client_digest.clone(),
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["eglot.el".to_string()],
        external_package: None,
        source_tree: Some(SourceTreeIdentity {
            source_repo_url: "https://github.com/emacs-mirror/emacs".to_string(),
            commit: SOURCE_EGLOT_COMMIT.to_string(),
            tree_sha1: SOURCE_EGLOT_TREE.to_string(),
        }),
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: format!(
                "https://fan-in.example/{SOURCE_EGLOT_COMMIT}/lisp/progmodes/eglot.el"
            ),
            gnu_tarball_sha256: client_digest,
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

/// The released MELPA Stable lsp-mode 10.0.0 class with fixture digests.
fn released_lsp_mode_row(client_digest: String, archive_digest: String) -> SubjectRow {
    SubjectRow {
        subject_id: RELEASED_LSP_MODE_ID.to_string(),
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::Released,
        emacs_release_tag: RELEASED_LSP_MODE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "10.0.0".to_string(),
        client_source_relative_path: "lsp-mode.el".to_string(),
        client_source_sha256: client_digest,
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["lsp-mode.el".to_string()],
        external_package: Some(ExternalPackageIdentity {
            archive_url: "https://stable.melpa.org/packages/fan-in-lsp-mode-fixture.tar"
                .to_string(),
            archive_sha256: archive_digest.clone(),
            attested_source_commit: RELEASED_LSP_MODE_COMMIT.to_string(),
            package_requires: vec!["emacs 28.1".to_string(), "fixture-dep 1.0".to_string()],
            minimum_emacs: "28.1".to_string(),
            checksum_disposition: "fan_in_fixture_archive_sha256".to_string(),
        }),
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://stable.melpa.org/packages/fan-in-lsp-mode-fixture.tar"
                .to_string(),
            gnu_tarball_sha256: archive_digest,
            observed_client_version_header: "10.0.0".to_string(),
        },
    }
}

/// The pinned upstream-source lsp-mode class with a fixture digest.
fn source_lsp_mode_row(client_digest: String) -> SubjectRow {
    SubjectRow {
        subject_id: SOURCE_LSP_MODE_ID.to_string(),
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: SOURCE_LSP_MODE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "10.0.1".to_string(),
        client_source_relative_path: "lsp-mode.el".to_string(),
        client_source_sha256: client_digest.clone(),
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["lsp-mode.el".to_string()],
        external_package: None,
        source_tree: Some(SourceTreeIdentity {
            source_repo_url: "https://github.com/emacs-lsp/lsp-mode".to_string(),
            commit: SOURCE_LSP_MODE_COMMIT.to_string(),
            tree_sha1: SOURCE_LSP_MODE_TREE.to_string(),
        }),
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: format!("https://fan-in.example/{SOURCE_LSP_MODE_COMMIT}/lsp-mode.el"),
            gnu_tarball_sha256: client_digest,
            observed_client_version_header: "10.0.1".to_string(),
        },
    }
}

/// The six denominator classes with fixture digests but real identity
/// shapes: ids, kinds, states, tags, tokens, headers, and commit/tree pins
/// mirror the checked manifest rows the per-family suites pin.
fn fixture_manifest() -> SubjectManifest {
    SubjectManifest {
        schema_version: xtask::emacs_subject_manifest::MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![
            bundled_row(
                BUNDLED_29_ID,
                "emacs-29.4",
                "29.4",
                "1.12.29",
                sha256_of(EMACS_29_EGLOT_BYTES),
            ),
            bundled_row(
                BUNDLED_30_ID,
                "emacs-30.1",
                "30.1",
                "1.17.30",
                sha256_of(EMACS_30_EGLOT_BYTES),
            ),
            released_eglot_row(sha256_of(RELEASED_EGLOT_BYTES), sha256_of(RELEASED_EGLOT_ARCHIVE)),
            source_eglot_row(sha256_of(SOURCE_EGLOT_BYTES)),
            released_lsp_mode_row(
                sha256_of(RELEASED_LSP_MODE_BYTES),
                sha256_of(RELEASED_LSP_MODE_ARCHIVE),
            ),
            source_lsp_mode_row(sha256_of(SOURCE_LSP_MODE_BYTES)),
        ],
    }
}

/// A fake exact Emacs installation: `<root>/bin/emacs` plus the bundled
/// client file under the real in-tree location.
fn fixture_installation(
    root: &Path,
    emacs_version: &str,
    client_bytes: &[u8],
    executable_bytes: &[u8],
) -> Result<PathBuf> {
    let bin = root.join("bin");
    let progmodes = root.join("share/emacs").join(emacs_version).join("lisp/progmodes");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&progmodes)?;
    let emacs = bin.join("emacs");
    fs::write(&emacs, executable_bytes)?;
    fs::write(progmodes.join("eglot.el"), client_bytes)?;
    Ok(emacs)
}

/// One bounded explicit-input directory for an external subject: the exact
/// client file, plus the exact package archive when requested.
#[allow(clippy::too_many_arguments)]
fn fixture_input_dir(
    root: &Path,
    name: &str,
    file_name: &str,
    client_bytes: &[u8],
    archive_name: Option<&str>,
    archive_bytes: Option<&[u8]>,
) -> Result<PathBuf> {
    let dir = root.join("materialized").join(name);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(file_name), client_bytes)?;
    if let (Some(name), Some(bytes)) = (archive_name, archive_bytes) {
        fs::write(dir.join(name), bytes)?;
    }
    Ok(dir)
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
// Denominator law
// ---------------------------------------------------------------------------

/// Positive control: the checked manifest completes the subject denominator
/// exactly, and the denominator itself is the issue's six-class catalog —
/// distinct ids, two bundled generations, a released/source pair per
/// external family, every pin exact.
#[test]
fn complete_denominator_validates_over_the_checked_manifest() -> Result<()> {
    // Denominator self-coherence: the issue catalog shape.
    let ids: Vec<&str> = SUBJECT_DENOMINATOR.iter().map(|slot| slot.subject_id).collect();
    let unique_ids: BTreeSet<&str> = ids.iter().copied().collect();
    ensure!(ids.len() == 6 && unique_ids.len() == 6, "six distinct subject classes, got {ids:?}");
    for kind in [
        SubjectClientKind::BundledEglot,
        SubjectClientKind::ExternalEglot,
        SubjectClientKind::LspMode,
    ] {
        let family: Vec<_> =
            SUBJECT_DENOMINATOR.iter().filter(|slot| slot.client_kind == kind).collect();
        ensure!(family.len() == 2, "family {kind:?} must bind exactly two classes");
        let states: Vec<_> = family.iter().map(|slot| slot.source_state).collect();
        match kind {
            SubjectClientKind::BundledEglot => {
                ensure!(
                    states.iter().all(|state| *state == ClientSourceState::Bundled),
                    "the bundled family binds only bundled generations"
                );
                ensure!(
                    family[0].emacs_release_tag != family[1].emacs_release_tag
                        && family[0].emacs_version_token != family[1].emacs_version_token,
                    "the two bundled generations bind different Emacs builds"
                );
            }
            SubjectClientKind::ExternalEglot | SubjectClientKind::LspMode => {
                ensure!(
                    states.contains(&ClientSourceState::Released)
                        && states.contains(&ClientSourceState::UpstreamSource),
                    "external family {kind:?} binds one released and one pinned-source class"
                );
                ensure!(
                    family[0].emacs_release_tag != family[1].emacs_release_tag,
                    "released and source classes of {kind:?} bind different exact pins"
                );
            }
        }
    }

    // The real checked manifest completes the denominator.
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    validate_subject_lane_denominator(&manifest)
        .expect("the checked manifest must complete the subject denominator exactly");
    let manifest_ids: BTreeSet<&str> =
        manifest.subjects.iter().map(|row| row.subject_id.as_str()).collect();
    ensure!(
        manifest_ids == unique_ids,
        "the checked manifest and the denominator must bind the same six ids: {manifest_ids:?}"
    );
    Ok(())
}

/// Opposite control: a partial denominator is never rendered complete. Every
/// five-row subset of the six must fail, naming the missing slot; and the
/// fan-in executes no missing subject work — the manifest still has five
/// rows after the refusal, so a missing row was failed, not materialized.
#[test]
fn partial_denominators_are_refused_slot_by_slot() {
    for missing in SUBJECT_DENOMINATOR {
        let mut partial = fixture_manifest();
        partial.subjects.retain(|row| row.subject_id != missing.subject_id);
        assert_eq!(partial.subjects.len(), 5, "the partial fixture holds five rows");
        let failure = validate_subject_lane_denominator(&partial)
            .expect_err("a partial denominator must never validate as complete");
        match &failure {
            SubjectFanInFailure::MissingSubject { slot_id } => {
                assert_eq!(*slot_id, missing.subject_id, "the refusal must name the missing slot")
            }
            other => panic!("expected MissingSubject, got {other:?}"),
        }
        // Non-builder: the refusal repaired nothing.
        assert_eq!(partial.subjects.len(), 5, "validation must not synthesize the missing row");
        assert!(
            partial.subjects.iter().all(|row| row.subject_id != missing.subject_id),
            "the missing row stays missing after the refusal"
        );
    }
}

/// Wrong-subject control: a manifest row citing a generation the
/// denominator does not bind — a prospective newer release arriving as a
/// surplus row — is refused as unbound, not adopted as surplus evidence.
#[test]
fn unbound_generation_rows_are_refused() {
    let mut surplus = fixture_manifest();
    // A prospective newer Eglot release shaped like a valid released row:
    // schema-valid, digest-consistent, and outside the bound denominator.
    let mut newer = released_eglot_row(
        sha256_of(b"prospective eglot 1.25 bytes"),
        sha256_of(b"prospective eglot 1.25 archive"),
    );
    newer.subject_id = "released_eglot_gnu_elpa_1_25".to_string();
    newer.emacs_release_tag = "1111111111111111111111111111111111111111".to_string();
    newer.client_version_hint = "1.25".to_string();
    newer.digest_audit.observed_client_version_header = "1.25".to_string();
    if let Some(package) = newer.external_package.as_mut() {
        package.attested_source_commit = newer.emacs_release_tag.clone();
    }
    surplus.subjects.push(newer);
    let failure = validate_subject_lane_denominator(&surplus)
        .expect_err("an unbound generation row must fail the fan-in");
    match &failure {
        SubjectFanInFailure::UnboundGeneration { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_25");
            assert!(
                reason.contains("reviewed change"),
                "the refusal must name the revision route: {reason}"
            );
        }
        other => panic!("expected UnboundGeneration, got {other:?}"),
    }
}

/// Duplicate control (PR review finding): a seventh row reusing a bound
/// subject id — even one whose copy is stale, hiding behind the first
/// match — must never certify. The fan-in law rejects duplicates
/// independently of the schema-level duplicate rejection, because it
/// certifies manifests it did not load through `SubjectManifest::load`.
#[test]
fn duplicate_rows_under_a_bound_id_are_refused() {
    // A stale duplicate of the bundled 30.1 row appended after the six
    // fixture rows: the completeness loop would match the first (bound)
    // copy, and without duplicate rejection the stale copy would ride
    // along unnoticed.
    let mut duplicated = fixture_manifest();
    let stale_copy = bundled_row(
        BUNDLED_30_ID,
        "emacs-31.1",
        "31.1",
        "1.18.0",
        sha256_of(b"a silently re-pinned bundled copy"),
    );
    duplicated.subjects.push(stale_copy);
    let failure = validate_subject_lane_denominator(&duplicated)
        .expect_err("a duplicate row under a bound id must fail the fan-in");
    match &failure {
        SubjectFanInFailure::DuplicateSubjectRow { subject_id } => {
            assert_eq!(subject_id, BUNDLED_30_ID);
        }
        other => panic!("expected DuplicateSubjectRow, got {other:?}"),
    }

    // A duplicate under an unbound id is equally refused: the first
    // surplus occurrence already fails as an unbound generation, so
    // repetition cannot bypass the refusal.
    let mut duplicated_unbound = fixture_manifest();
    let mut extra = released_eglot_row(
        sha256_of(b"prospective eglot 1.25 bytes"),
        sha256_of(b"prospective eglot 1.25 archive"),
    );
    extra.subject_id = "released_eglot_gnu_elpa_1_25".to_string();
    duplicated_unbound.subjects.push(extra.clone());
    duplicated_unbound.subjects.push(extra);
    let failure = validate_subject_lane_denominator(&duplicated_unbound)
        .expect_err("a duplicated unbound row must fail the fan-in");
    assert!(
        matches!(
            &failure,
            SubjectFanInFailure::UnboundGeneration { subject_id, .. }
                if subject_id == "released_eglot_gnu_elpa_1_25"
        ),
        "expected UnboundGeneration on the first surplus occurrence, got {failure:?}"
    );
}

/// Stale control: a bound subject id whose row binds a different generation
/// — a newer release silently re-pinned under the existing id, or a drifted
/// host token — is refused until the denominator is revised. Drift in any
/// bound field is named.
#[test]
fn stale_generation_under_a_bound_id_is_refused() {
    // A newer release re-pinned under the released Eglot id (tag and header
    // drift together): exactly the silent relabel the issue forbids.
    let mut repinned = fixture_manifest();
    let row = repinned
        .subjects
        .iter_mut()
        .find(|row| row.subject_id == RELEASED_EGLOT_ID)
        .expect("released eglot fixture row");
    row.emacs_release_tag = "2222222222222222222222222222222222222222".to_string();
    let package = row.external_package.as_mut().expect("package identity");
    package.attested_source_commit = row.emacs_release_tag.clone();
    row.client_version_hint = "1.25".to_string();
    row.digest_audit.observed_client_version_header = "1.25".to_string();
    let failure = validate_subject_lane_denominator(&repinned)
        .expect_err("a re-pinned generation under a bound id must fail the fan-in");
    match &failure {
        SubjectFanInFailure::StaleSubjectRow { subject_id, reason } => {
            assert_eq!(subject_id, RELEASED_EGLOT_ID);
            assert!(
                reason.contains("release tag") && reason.contains("version header"),
                "the refusal must name the drifted fields: {reason}"
            );
        }
        other => panic!("expected StaleSubjectRow, got {other:?}"),
    }

    // Token-only drift under a bundled id is still stale.
    let mut drifted_token = fixture_manifest();
    let row = drifted_token
        .subjects
        .iter_mut()
        .find(|row| row.subject_id == BUNDLED_30_ID)
        .expect("bundled 30.1 fixture row");
    row.emacs_version_token = "31.1".to_string();
    let failure = validate_subject_lane_denominator(&drifted_token)
        .expect_err("a drifted host token under a bound id must fail the fan-in");
    match &failure {
        SubjectFanInFailure::StaleSubjectRow { subject_id, reason } => {
            assert_eq!(subject_id, BUNDLED_30_ID);
            assert!(reason.contains("host token"), "the refusal must name the token: {reason}");
        }
        other => panic!("expected StaleSubjectRow, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The coherent six-subject proof
// ---------------------------------------------------------------------------

/// The one coherent fan-in proof: all six denominator subjects resolve
/// through the landed resolver against one bounded materialization, bind
/// their exact identities, coexist in one bounded cache location under six
/// distinct complete identities, and reuse their entries deterministically.
#[test]
fn all_denominator_subjects_resolve_in_one_coherent_proof() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs_29 = fixture_installation(
        &root.path().join("install-29"),
        "29.4",
        EMACS_29_EGLOT_BYTES,
        b"fake exact emacs 29.4 executable",
    )?;
    let emacs_30 = fixture_installation(
        &root.path().join("install-30"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        b"fake exact emacs 30.1 executable",
    )?;
    let eglot_released_input = fixture_input_dir(
        root.path(),
        "eglot-released",
        "eglot.el",
        RELEASED_EGLOT_BYTES,
        Some("eglot-1.24.tar"),
        Some(RELEASED_EGLOT_ARCHIVE),
    )?;
    let eglot_source_input =
        fixture_input_dir(root.path(), "eglot-source", "eglot.el", SOURCE_EGLOT_BYTES, None, None)?;
    let lsp_mode_released_input = fixture_input_dir(
        root.path(),
        "lsp-mode-released",
        "lsp-mode.el",
        RELEASED_LSP_MODE_BYTES,
        Some("lsp-mode-10.0.0.tar"),
        Some(RELEASED_LSP_MODE_ARCHIVE),
    )?;
    let lsp_mode_source_input = fixture_input_dir(
        root.path(),
        "lsp-mode-source",
        "lsp-mode.el",
        SOURCE_LSP_MODE_BYTES,
        None,
        None,
    )?;

    let manifest = fixture_manifest();
    let cache_root = root.path().join("fan-in-cache");

    // Bind the exact input paths before building the borrowed requests.
    let released_eglot_file = eglot_released_input.join("eglot.el");
    let released_eglot_archive = eglot_released_input.join("eglot-1.24.tar");
    let source_eglot_file = eglot_source_input.join("eglot.el");
    let released_lsp_mode_file = lsp_mode_released_input.join("lsp-mode.el");
    let released_lsp_mode_archive = lsp_mode_released_input.join("lsp-mode-10.0.0.tar");
    let source_lsp_mode_file = lsp_mode_source_input.join("lsp-mode.el");

    let requests: Vec<(&str, ResolveRequest<'_>)> = vec![
        (
            BUNDLED_29_ID,
            ResolveRequest {
                emacs_executable: &emacs_29,
                client_source: None,
                client_package: None,
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
            },
        ),
        (
            BUNDLED_30_ID,
            ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: None,
                client_package: None,
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
        ),
        (
            RELEASED_EGLOT_ID,
            ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&released_eglot_file),
                client_package: Some(&released_eglot_archive),
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
        ),
        (
            SOURCE_EGLOT_ID,
            ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&source_eglot_file),
                client_package: None,
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
        ),
        (
            RELEASED_LSP_MODE_ID,
            ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&released_lsp_mode_file),
                client_package: Some(&released_lsp_mode_archive),
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
        ),
        (
            SOURCE_LSP_MODE_ID,
            ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&source_lsp_mode_file),
                client_package: None,
                cache_root: &cache_root,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
        ),
    ];
    assert_eq!(requests.len(), SUBJECT_DENOMINATOR.len(), "one request per denominator slot");

    // First pass: every denominator subject resolves with its identity
    // intact; the six cache keys are pairwise distinct.
    let mut seen_keys = BTreeSet::new();
    for (expected_slot, (subject_id, request)) in SUBJECT_DENOMINATOR.iter().zip(requests.iter()) {
        let resolved = resolve(&manifest, subject_id, request).with_context(|| {
            format!("denominator subject {subject_id} must resolve in the coherent proof")
        })?;
        ensure!(resolved.subject_id == expected_slot.subject_id);
        let client = &resolved.client;
        ensure!(
            client.client_id == expected_slot.subject_id
                && client.kind == expected_slot.client_kind.runner_kind()
                && client.version == expected_slot.client_version_hint
                && client.source_state == expected_slot.source_state
                && client.source_ref == expected_slot.emacs_release_tag,
            "resolved identity fields must come from the bound slot {subject_id}"
        );
        // Package identity exactly for the released classes.
        let expects_package = expected_slot.source_state == ClientSourceState::Released;
        ensure!(
            resolved.client_package.is_some() == expects_package
                && client.package_sha256.is_some() == expects_package,
            "released classes carry the validated archive, source and bundled classes carry none: \
             {subject_id}"
        );
        // The complete declared external identity keys exactly the four
        // external classes, read back from the immutable cache record.
        let record_path = resolved.cache_entry.join(CACHE_ENTRY_FILE);
        let record: SubjectInputRecord = serde_json::from_slice(&fs::read(&record_path)?)
            .with_context(|| format!("parsing the cache record of {subject_id}"))?;
        let is_bundled = expected_slot.client_kind == SubjectClientKind::BundledEglot;
        ensure!(
            record.identity.external_identity_sha256.is_some() != is_bundled,
            "the declared external identity is bound exactly for the four external classes: \
             {subject_id}"
        );
        ensure!(
            record.identity.emacs_version_token == expected_slot.emacs_version_token,
            "the cached identity binds the slot's host token: {subject_id}"
        );
        ensure!(!resolved.reused_cache, "first resolution writes a fresh entry: {subject_id}");
        ensure!(
            seen_keys.insert(resolved.cache_key.clone()),
            "cache keys must be pairwise distinct across the denominator: collision on {subject_id}"
        );
    }
    ensure!(seen_keys.len() == 6, "six distinct cache keys, got {}", seen_keys.len());

    // The bounded cache location holds exactly the six entries, each only
    // its record file.
    let mut entry_dirs = fs::read_dir(&cache_root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    entry_dirs.sort();
    ensure!(entry_dirs.len() == 6, "six cache entries, got {}", entry_dirs.len());
    for dir in &entry_dirs {
        let files: Vec<String> = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        ensure!(
            files.iter().all(|name| name == CACHE_ENTRY_FILE) && files.len() == 1,
            "each entry holds only its record: {files:?}"
        );
    }

    // Second pass over the identical inputs: deterministic reuse for the
    // whole denominator.
    for (subject_id, request) in requests.iter() {
        let resolved = resolve(&manifest, subject_id, request)?;
        ensure!(resolved.reused_cache, "repeat resolution must reuse the entry: {subject_id}");
    }
    Ok(())
}

/// Fault control: substituted material is a typed rejection through the same
/// resolver, never a pass — cross-generation bundled pairing, source/release
/// byte resemblance under one version header, cross-family bytes under a
/// foreign subject, and package input to a source subject.
#[test]
fn substituted_material_is_typed_rejected_across_families_and_generations() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs_29 = fixture_installation(
        &root.path().join("install-29"),
        "29.4",
        EMACS_29_EGLOT_BYTES,
        b"fake exact emacs 29.4 executable",
    )?;
    let emacs_30 = fixture_installation(
        &root.path().join("install-30"),
        "30.1",
        EMACS_30_EGLOT_BYTES,
        b"fake exact emacs 30.1 executable",
    )?;
    let eglot_released_input = fixture_input_dir(
        root.path(),
        "eglot-released",
        "eglot.el",
        RELEASED_EGLOT_BYTES,
        Some("eglot-1.24.tar"),
        Some(RELEASED_EGLOT_ARCHIVE),
    )?;
    // Source Eglot bytes offered to the released Eglot subject: the same
    // `1.24` header, different bytes.
    let eglot_source_input =
        fixture_input_dir(root.path(), "eglot-source", "eglot.el", SOURCE_EGLOT_BYTES, None, None)?;
    // Released Eglot bytes under the lsp-mode file name, offered to the
    // released lsp-mode subject with the correct lsp-mode archive.
    let cross_family_input = fixture_input_dir(
        root.path(),
        "cross-family",
        "lsp-mode.el",
        RELEASED_EGLOT_BYTES,
        Some("lsp-mode-10.0.0.tar"),
        Some(RELEASED_LSP_MODE_ARCHIVE),
    )?;
    // The source lsp-mode subject's exact bytes plus a released archive:
    // correct file digest, forbidden package input.
    let lsp_mode_source_with_archive = fixture_input_dir(
        root.path(),
        "lsp-mode-source-plus-package",
        "lsp-mode.el",
        SOURCE_LSP_MODE_BYTES,
        Some("lsp-mode-10.0.0.tar"),
        Some(RELEASED_LSP_MODE_ARCHIVE),
    )?;

    let manifest = fixture_manifest();
    let cache = root.path().join("typed-rejection-cache");

    // Bind the exact input paths before building the borrowed requests.
    let released_eglot_archive = eglot_released_input.join("eglot-1.24.tar");
    let source_eglot_file = eglot_source_input.join("eglot.el");
    let cross_family_file = cross_family_input.join("lsp-mode.el");
    let cross_family_archive = cross_family_input.join("lsp-mode-10.0.0.tar");
    let source_plus_file = lsp_mode_source_with_archive.join("lsp-mode.el");
    let source_plus_archive = lsp_mode_source_with_archive.join("lsp-mode-10.0.0.tar");

    struct Case<'a> {
        subject_id: &'static str,
        request: ResolveRequest<'a>,
        expect: fn(&str, &str) -> bool,
    }
    let cases = vec![
        // Cross-generation bundled pairing: the 29.4 installation satisfies
        // only the 29.4 subject.
        Case {
            subject_id: BUNDLED_30_ID,
            request: ResolveRequest {
                emacs_executable: &emacs_29,
                client_source: None,
                client_package: None,
                cache_root: &cache,
                probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
            },
            expect: |subject_id, reason| {
                subject_id == BUNDLED_30_ID && reason.contains("does not match the pinned")
            },
        },
        // Source bytes offered to the released subject: the header
        // resemblance is not identity.
        Case {
            subject_id: RELEASED_EGLOT_ID,
            request: ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&source_eglot_file),
                client_package: Some(&released_eglot_archive),
                cache_root: &cache,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
            expect: |subject_id, reason| {
                subject_id == RELEASED_EGLOT_ID && reason.contains("does not match the pinned")
            },
        },
        // Cross-family bytes under the lsp-mode subject.
        Case {
            subject_id: RELEASED_LSP_MODE_ID,
            request: ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&cross_family_file),
                client_package: Some(&cross_family_archive),
                cache_root: &cache,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
            expect: |subject_id, reason| {
                subject_id == RELEASED_LSP_MODE_ID && reason.contains("does not match the pinned")
            },
        },
        // Package input to a source subject: states are non-interchangeable.
        Case {
            subject_id: SOURCE_LSP_MODE_ID,
            request: ResolveRequest {
                emacs_executable: &emacs_30,
                client_source: Some(&source_plus_file),
                client_package: Some(&source_plus_archive),
                cache_root: &cache,
                probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
            },
            expect: |subject_id, reason| {
                subject_id == SOURCE_LSP_MODE_ID && reason.contains("non-interchangeable")
            },
        },
    ];
    assert!(
        !cache.exists(),
        "typed rejections must leave no cache state behind (checked per case below)"
    );
    for case in cases {
        let failure = resolve(&manifest, case.subject_id, &case.request)
            .expect_err("substituted material must be rejected, never resolved");
        match rejection_of(&failure) {
            SubjectRejection::IdentityMismatch { subject_id, reason } => assert!(
                (case.expect)(subject_id, reason),
                "unexpected identity-mismatch shape for {}: {reason}",
                case.subject_id
            ),
            other => panic!("expected IdentityMismatch for {}, got {other:?}", case.subject_id),
        }
        assert!(
            !cache.exists(),
            "a rejected resolution must not write cache state for {}",
            case.subject_id
        );
    }
    Ok(())
}

/// Consumer-facing disposition table across the denominator: released
/// classes require their package input, source and bundled classes refuse
/// one, and the subjects whose driver adapters do not exist yet keep their
/// typed launch refusals at the host-run boundary — materialization is not
/// a journey claim.
#[test]
fn launch_refusals_are_preserved_across_the_denominator() {
    for slot in SUBJECT_DENOMINATOR {
        let subject = EmacsClientSubject::from_id(slot.subject_id)
            .expect("every denominator slot dispatches through the registry");
        let expects_package = slot.source_state == ClientSourceState::Released;
        assert_eq!(
            subject.requires_client_package(),
            expects_package,
            "package-input requirement must match the source state for {}",
            slot.subject_id
        );
        // The launch table: the two bundled generations, the released
        // Eglot 1.24 row, and — since #8776's external adapter earned its
        // package-free launch — the pinned upstream-source Eglot row have
        // driver adapters; both lsp-mode rows materialize but refuse to
        // launch.
        let launchable = matches!(
            slot.subject_id,
            "bundled_eglot_emacs_29_4"
                | "bundled_eglot_emacs_30_1"
                | "released_eglot_gnu_elpa_1_24"
                | "source_eglot_emacs_c1ad9d27"
        );
        assert_eq!(
            subject.launches_with_current_driver(),
            launchable,
            "the launch table must stay typed across the denominator for {}",
            slot.subject_id
        );
        if !launchable {
            let run = emacs_host_run::EmacsHostRunInputs {
                emacs_executable: PathBuf::from("/nonexistent/emacs"),
                candidate_executable: PathBuf::from("/nonexistent/perllsp"),
                client_source: PathBuf::from("/nonexistent/client.el"),
                client_package: None,
                out_root: PathBuf::from("/nonexistent/out"),
                timeout_ms: 0,
            };
            let error = emacs_host_run::host_run(Path::new("/nonexistent/repo"), subject, &run)
                .err()
                .expect("an unadapted subject must refuse before any launch step");
            assert!(
                error.to_string().contains("no driver adapter yet"),
                "the refusal must name the missing-adapter boundary for {}: {error}",
                slot.subject_id
            );
        }
    }
}
