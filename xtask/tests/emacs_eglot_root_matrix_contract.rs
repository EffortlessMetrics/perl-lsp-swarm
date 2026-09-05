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
        "(require 'eglot)",
        "(project-current nil)",
        "(project-root project)",
        "(eglot--current-project)",
    ] {
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

/// Scan the driver's receipt payload for the keys it actually emits.
///
/// The payload is a backquoted alist, so every emitted slot appears as an
/// `(ident . ,value)` pair. Any other parenthesized form in the region is a
/// call whose head is followed by an argument rather than a dot, so it is
/// skipped without a hand-maintained exclusion list.
fn driver_receipt_slots(driver: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let call = driver.find("(perl-lsp-root-probe--record").ok_or("driver has no receipt call")?;
    let end =
        driver.find("(defun perl-lsp-root-probe--record").ok_or("driver has no receipt writer")?;
    let region = driver.get(call..end).ok_or("receipt payload region is malformed")?;

    let mut slots = Vec::new();
    for fragment in region.split('(').skip(1) {
        let ident: String = fragment
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        if ident.is_empty() {
            continue;
        }
        let rest = fragment[ident.len()..].trim_start();
        if rest.starts_with('.') && !slots.contains(&ident) {
            slots.push(ident);
        }
    }
    Ok(slots)
}

/// The record's declared slot vocabulary and the driver's emitted receipt
/// keys are one contract. Drift in either direction — a declared slot the
/// instrument never fills, or an undeclared key smuggled into a receipt —
/// silently corrupts the eventual hosted observation, so it fails here.
#[test]
fn declared_observation_slots_match_the_driver_receipt_keys() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;
    let record: Value = serde_json::from_str(&read(&root, RECORD)?)?;

    let mut declared: Vec<String> = record
        .pointer("/observation_slots/slots")
        .and_then(Value::as_array)
        .ok_or("observation_slots.slots missing")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    assert!(!declared.is_empty(), "the record must declare a closed slot vocabulary");

    let mut emitted = driver_receipt_slots(&driver)?;
    // Negative control on the scanner itself: a payload that parsed to
    // nothing would make the comparison below vacuously satisfiable.
    assert!(
        emitted.contains(&"case_id".to_owned()) && emitted.contains(&"driver_complete".to_owned()),
        "receipt-key scan found no recognizable payload: {emitted:?}"
    );

    declared.sort();
    emitted.sort();
    assert_eq!(
        emitted, declared,
        "driver receipt keys and declared observation slots must agree exactly"
    );
    Ok(())
}

/// The driver with every string literal and `;` comment blanked out.
///
/// Reads elisp the way the Emacs reader does for this purpose: string
/// contents, backslash escapes, character literals, and comments cannot
/// contribute structure or sentinels. What survives is real code, so the
/// checks below cannot be fooled by a parenthesis or a keyword that only
/// ever appears inside a docstring or a regexp.
fn driver_code_only(driver: &str) -> String {
    let source: Vec<char> = driver.chars().collect();
    let mut code = String::with_capacity(driver.len());
    let (mut in_string, mut in_comment) = (false, false);
    let mut index = 0;
    while index < source.len() {
        let current = source[index];
        let keep = if in_string {
            match current {
                '\\' => {
                    index += 1;
                    false
                }
                '"' => {
                    in_string = false;
                    false
                }
                _ => false,
            }
        } else if in_comment {
            if current == '\n' {
                in_comment = false;
                true
            } else {
                false
            }
        } else {
            match current {
                // `?(` and `?\)` are characters, not structure.
                '?' if index + 1 < source.len() => {
                    index += if source[index + 1] == '\\' { 2 } else { 1 };
                    false
                }
                '"' => {
                    in_string = true;
                    false
                }
                ';' => {
                    in_comment = true;
                    false
                }
                _ => true,
            }
        };
        code.push(if keep { current } else { ' ' });
        index += 1;
    }
    code
}

/// Count of balanced top-level forms, or the offset of the first `)` that
/// closes more than was open.
fn driver_form_balance(code: &str) -> Result<usize, String> {
    let (mut depth, mut top_level_forms) = (0i32, 0usize);
    for (index, current) in code.char_indices() {
        match current {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err(format!("unbalanced `)` at character {index}"));
                }
                if depth == 0 {
                    top_level_forms += 1;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(format!("{depth} form(s) left open at end of driver"));
    }
    Ok(top_level_forms)
}

/// Keyword sentinels the receipt may carry.
///
/// `json-serialize` recognizes `t` for JSON true and these two keywords
/// for false and null. Every other keyword in a receipt value — `:true`
/// and `:json-false` are the tempting ones — is an ordinary symbol it
/// refuses outright, so the write signals instead of producing a receipt.
const RECEIPT_SENTINELS: [&str; 2] = [":false", ":null"];

/// A refusal that cannot be written is not a refusal.
///
/// The typed-refusal branches are exactly the paths that run when no
/// session exists, so an unserializable sentinel there means the honest
/// outcomes — missing candidate, failed connect, unreadable initialize
/// evidence — produce nothing at all, while the success path still
/// writes. The record's fail-closed claim depends on the opposite.
#[test]
fn driver_receipt_uses_only_serializable_sentinels() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let code = driver_code_only(&read(&root, DRIVER)?);

    let mut used: Vec<String> = Vec::new();
    for (index, _) in code.match_indices(':') {
        let keyword: String = code[index..]
            .chars()
            .take_while(|c| *c == ':' || c.is_ascii_lowercase() || *c == '-')
            .collect();
        // `[[:space:]]` and friends live in strings, which are already
        // blanked; a bare `:` in code starts a keyword.
        if keyword.len() > 1 && !used.contains(&keyword) {
            used.push(keyword);
        }
    }
    // Negative control: the driver does serialize negative answers, so a
    // scan that found no keywords at all would pass vacuously.
    assert!(
        used.iter().any(|keyword| keyword == ":null"),
        "sentinel scan recovered no receipt keywords: {used:?}"
    );

    let unserializable: Vec<&String> =
        used.iter().filter(|keyword| !RECEIPT_SENTINELS.contains(&keyword.as_str())).collect();
    assert!(
        unserializable.is_empty(),
        "json-serialize accepts only {RECEIPT_SENTINELS:?} plus `t`; driver also uses {unserializable:?}"
    );
    Ok(())
}

#[test]
fn root_probe_driver_is_a_readable_elisp_program() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;
    let forms = driver_form_balance(&driver_code_only(&driver))
        .map_err(|error| format!("{DRIVER}: {error}"))?;
    // Negative control on the reader: a driver whose parentheses all hid
    // inside strings would balance trivially at zero forms.
    assert!(
        forms >= 10,
        "driver should read as its top-level requires and defuns, found {forms} forms"
    );
    Ok(())
}
