//! Contract suite for the #11747 stock Eglot/project.el root-matrix record.
//!
//! Claim boundary: the checked observation record must fail closed until an
//! exact pinned-subject host run fills it, the instrument must observe
//! without prebinding any expected root, and the recorded subject scope must
//! match the landed subject denominator rather than a hand-copied list.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const RECORD: &str = ".ci/editor-clients/eglot-project-root-matrix.v1.json";
const DRIVER: &str = "scripts/test/eglot-project-root-driver.el";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read(root: &Path, relative: &str) -> Result<String, Box<dyn Error>> {
    fs::read_to_string(root.join(relative))
        .map_err(|error| format!("missing checked artifact {relative}: {error}").into())
}

/// The canonical twelve-case matrix ids, in #11366 declaration order.
const REQUIRED_CASES: [&str; 12] = [
    "git_root",
    "makefile_pl_no_vcs",
    "build_pl_no_vcs",
    "cpanfile_no_vcs",
    "dist_ini_no_vcs",
    "perl_lsp_config_root",
    "nested_makefile_under_git",
    "nested_cpanfile_under_git",
    "outer_config_nested_distribution",
    "sibling_distributions",
    "git_worktree_shape",
    "standalone_file",
];

#[test]
fn root_matrix_record_covers_exactly_the_required_cases() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = read(&root, RECORD)?;
    let record: Value = serde_json::from_str(&text)?;
    assert_eq!(
        record.get("schema_version").and_then(Value::as_str),
        Some("emacs_eglot_project_root_observations.v1"),
        "record schema identity is fixed"
    );
    let cases = record.get("cases").and_then(Value::as_object).ok_or("cases object missing")?;
    let mut case_ids: Vec<&str> = cases.keys().map(String::as_str).collect();
    case_ids.sort_unstable();
    let mut required = REQUIRED_CASES.to_vec();
    required.sort_unstable();
    assert_eq!(case_ids, required, "case set must equal the 12-case matrix exactly");
    for (id, case) in cases {
        assert!(
            case.get("open_file_contract").and_then(Value::as_str).is_some_and(|open_file| {
                !open_file.is_empty() && !Path::new(open_file).is_absolute()
            }),
            "case {id} must bind a fixture-relative open-file contract"
        );
        // Fail-closed record: no row may carry observed facts it does not
        // have, and every pending row stays explicit about why.
        assert!(
            case.get("observations").and_then(Value::as_array).is_some_and(Vec::is_empty),
            "case {id} must stay unobserved until an actual host run"
        );
        assert_eq!(
            case.get("disposition").and_then(Value::as_str),
            Some("hosted_pending"),
            "case {id} disposition must be explicit hosted-pending"
        );
        assert!(
            case.get("disposition_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty()),
            "case {id} must carry a nonempty disposition reason"
        );
        assert!(
            case.get("evidence").is_none_or(Value::is_null),
            "case {id} may not claim evidence without a host run"
        );
    }
    Ok(())
}

#[test]
fn root_matrix_record_states_the_issue_falsifiers_as_binding_rules() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = read(&root, RECORD)?;
    let rules_text = text.to_ascii_lowercase();
    for falsifier in [
        "without prebinding the expected root",
        "never pre-populated",
        "working directory is never equated to or used as project root authority",
        "observed separately and are never inferred from one another",
        "missing instrumentation records unsupported-by-instrument refusal",
        "generation identity accompanies every observation",
    ] {
        assert!(
            rules_text.contains(&falsifier.to_ascii_lowercase()),
            "recording rules must state the falsifier verbatim: {falsifier}"
        );
    }
    for boundary in [
        "#11360/#11361 producer lanes",
        "no semantic root support cell",
        "custom project backend",
        "server-root heuristic",
    ] {
        assert!(
            text.contains(boundary),
            "claim boundary must forbid invented behavior: {boundary}"
        );
    }
    Ok(())
}

#[test]
fn root_matrix_subject_scope_matches_the_landed_denominator() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = read(&root, RECORD)?;
    let record: Value = serde_json::from_str(&text)?;

    use xtask::editor_client_compat::ClientSourceState;
    use xtask::emacs_subject_fan_in::SUBJECT_DENOMINATOR;
    use xtask::emacs_subject_manifest::SubjectClientKind;

    let eglot_slots: Vec<_> = SUBJECT_DENOMINATOR
        .iter()
        .filter(|slot| {
            matches!(
                slot.client_kind,
                SubjectClientKind::BundledEglot | SubjectClientKind::ExternalEglot
            )
        })
        .collect();
    let deferred_ids: Vec<&str> = eglot_slots
        .iter()
        .filter(|slot| slot.source_state == ClientSourceState::UpstreamSource)
        .map(|slot| slot.subject_id)
        .collect();

    let scoped: Vec<&str> = record
        .get("subjects_in_scope")
        .and_then(Value::as_array)
        .ok_or("subjects_in_scope missing")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let recorded_deferred: Vec<&str> = record
        .get("subjects_deferred")
        .and_then(Value::as_array)
        .ok_or("subjects_deferred missing")?
        .iter()
        .filter_map(|row| row.get("subject_id").and_then(Value::as_str))
        .collect();

    let expected_scoped: Vec<&str> = eglot_slots
        .iter()
        .filter(|slot| slot.source_state != ClientSourceState::UpstreamSource)
        .map(|slot| slot.subject_id)
        .collect();
    assert_eq!(scoped, expected_scoped, "scoped subjects must be denominator-derived");
    assert_eq!(
        recorded_deferred, deferred_ids,
        "deferred subjects must be exactly the upstream-source Eglot slots"
    );
    assert_eq!(scoped.len() + deferred_ids.len(), eglot_slots.len());
    Ok(())
}

#[test]
fn root_probe_driver_observes_stock_selection_without_prebinding() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;
    // The instrument must call the exact stock seams.
    for seam in [
        "(require 'eglot)", "(project-current nil)", "(project-root project)", "(eglot--current-project)"] {
        assert!(driver.contains(seam), "driver must observe through stock seam: {seam}");
    }
    // No-prebinding falsifiers: the instrument never injects, remembers, or
    // enumerates known roots.
    for forbidden in [
        "project-remember-project",
        "project-known-project-roots",
        "PERL_LSP_PROJECT_ROOT",
        "intended_root",
        "expected_root",
    ] {
        assert!(!driver.contains(forbidden), "driver must not prebind via {forbidden}");
    }
    // Negative answers must survive serialization as native sentinels so
    // non-recognition stays a recorded fact instead of corrupting the JSON.
    assert!(driver.contains(":false"), "driver needs the native false sentinel");
    assert!(driver.contains(":null"), "driver needs the native null sentinel");
    assert!(!driver.contains(":json-false"), "legacy json.el sentinel is not accepted");
    Ok(())
}

#[test]
fn root_probe_driver_refuses_to_pass_without_cleanup_proof() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;
    // Cleanup verification happens after shutdown and drives the receipt:
    // live processes fail closed, and both cleanup slots are always present.
    assert!(
        driver.contains("process-live-p") && driver.contains("--live-server-count server"),
        "driver must verify server liveness during cleanup"
    );
    assert!(
        driver.contains("live server process behind"),
        "driver must fail closed on surviving sessions"
    );
    for slot in [
        "process_cleanup_live_servers",
        "cleanup_buffer_closed",
        "driver_complete",
        "generation_identity",
        "subject_id",
    ] {
        assert!(driver.contains(slot), "receipt must carry the {slot} cleanup slot");
    }
    // A missing candidate records a manual action with a typed refusal; it
    // can never degrade into an invented session fact.
    assert!(
        driver.contains("candidate_executable_not_supplied")
            && driver.contains("generation_identity"),
        "absent candidate is a typed refusal, not a pass"
    );
    assert!(
        driver.contains("manual_action_required"),
        "the receipt separates manual action from session facts"
    );
    Ok(())
}

#[test]
fn root_matrix_record_and_driver_paths_are_the_declared_instrument() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let text = read(&root, RECORD)?;
    let record: Value = serde_json::from_str(&text)?;
    let declared_driver = record
        .pointer("/instrument/driver")
        .and_then(Value::as_str)
        .ok_or("instrument.driver missing")?;
    assert_eq!(declared_driver, DRIVER);
    assert!(root.join(declared_driver).is_file(), "declared instrument driver must exist on disk");
    let fixtures = record
        .pointer("/instrument/fixtures")
        .and_then(Value::as_str)
        .ok_or("instrument.fixtures missing")?;
    assert!(fixtures.contains("#11366"), "fixtures reference must consume #11366 unchanged");
    Ok(())
}
