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
    // Batch Emacs cannot answer `yes-or-no-p': killing a buffer something
    // marked modified would block on a prompt and hang the run, which is the
    // one failure mode a fail-closed instrument must not have.
    let kill = driver.find("(kill-buffer buffer)").ok_or("driver must release the probe buffer")?;
    let clears_flag = driver.find("(set-buffer-modified-p nil)").is_some_and(|clear| clear < kill);
    assert!(
        clears_flag,
        "the probe buffer's modified flag must be cleared before `kill-buffer', or a batch run \
         blocks on an interactive prompt"
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

/// Minimal s-expression shape reader for the driver.
///
/// The rest of this suite binds the driver textually, which cannot see
/// whether a branch is actually reachable: a `condition-case` that closes
/// before its handlers still contains every expected token. These few
/// structural checks give the suite an oracle that textual `contains`
/// assertions cannot provide short of an Emacs host.
#[derive(Debug, Clone)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

impl Sexp {
    fn head(&self) -> Option<&str> {
        match self {
            Sexp::List(items) => match items.first() {
                Some(Sexp::Atom(name)) => Some(name.as_str()),
                _ => None,
            },
            Sexp::Atom(_) => None,
        }
    }

    fn nth_atom(&self, index: usize) -> Option<&str> {
        match self {
            Sexp::List(items) => match items.get(index) {
                Some(Sexp::Atom(name)) => Some(name.as_str()),
                _ => None,
            },
            Sexp::Atom(_) => None,
        }
    }

    /// Depth-first walk, parents before children.
    fn walk<'a>(&'a self, out: &mut Vec<&'a Sexp>) {
        out.push(self);
        if let Sexp::List(items) = self {
            for item in items {
                item.walk(out);
            }
        }
    }
}

/// Read every top-level form. Quote/backquote/unquote prefixes are dropped:
/// they do not change the branch shape these checks assert.
fn parse_elisp(source: &str) -> Result<Vec<Sexp>, Box<dyn Error>> {
    let bytes: Vec<char> = source.chars().collect();
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];
    let mut index = 0usize;
    while index < bytes.len() {
        let current = bytes[index];
        match current {
            ';' => {
                while index < bytes.len() && bytes[index] != '\n' {
                    index += 1;
                }
            }
            c if c.is_whitespace() => index += 1,
            '\'' | '`' | ',' => {
                index += 1;
                if bytes.get(index) == Some(&'@') {
                    index += 1;
                }
            }
            '?' => {
                // Character literal: `?x` or `?\x`.
                index += if bytes.get(index + 1) == Some(&'\\') { 3 } else { 2 };
            }
            '"' => {
                index += 1;
                let mut literal = String::from("\"");
                while index < bytes.len() && bytes[index] != '"' {
                    if bytes[index] == '\\' {
                        index += 1;
                        if index >= bytes.len() {
                            break;
                        }
                    }
                    literal.push(bytes[index]);
                    index += 1;
                }
                index += 1;
                literal.push('"');
                let depth = stack.len();
                stack
                    .get_mut(depth - 1)
                    .ok_or("elisp reader lost its frame")?
                    .push(Sexp::Atom(literal));
            }
            '(' => {
                stack.push(Vec::new());
                index += 1;
            }
            ')' => {
                let finished = stack.pop().ok_or("unbalanced ')' in driver source")?;
                if stack.is_empty() {
                    return Err("unbalanced ')' in driver source".into());
                }
                let depth = stack.len();
                stack
                    .get_mut(depth - 1)
                    .ok_or("elisp reader lost its frame")?
                    .push(Sexp::List(finished));
                index += 1;
            }
            _ => {
                let start = index;
                while index < bytes.len()
                    && !bytes[index].is_whitespace()
                    && !matches!(bytes[index], '(' | ')' | '"' | ';')
                {
                    index += 1;
                }
                let atom: String = bytes[start..index].iter().collect();
                let depth = stack.len();
                stack
                    .get_mut(depth - 1)
                    .ok_or("elisp reader lost its frame")?
                    .push(Sexp::Atom(atom));
            }
        }
    }
    if stack.len() != 1 {
        return Err("unbalanced '(' in driver source".into());
    }
    stack.pop().ok_or_else(|| "elisp reader lost its frame".into())
}

fn driver_forms(root: &Path) -> Result<Vec<Sexp>, Box<dyn Error>> {
    let driver = read(root, DRIVER)?;
    let top = parse_elisp(&driver)?;
    let mut all = Vec::new();
    for form in &top {
        form.walk(&mut all);
    }
    Ok(all.into_iter().cloned().collect())
}

#[test]
fn root_probe_driver_session_branches_are_reachable() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let forms = driver_forms(&root)?;

    // `(if candidate THEN ELSE)` — exactly three subforms. A fourth means the
    // guarding `condition-case` closed early and the success path, the typed
    // refusal, or both silently became stray else-forms.
    let candidate_branch = forms
        .iter()
        .find(|form| form.head() == Some("if") && form.nth_atom(1) == Some("candidate"))
        .ok_or("driver must branch on the supplied candidate")?;
    let Sexp::List(candidate_items) = candidate_branch else {
        return Err("candidate branch is not a form".into());
    };
    assert_eq!(
        candidate_items.len(),
        4,
        "`(if candidate ...)` must have exactly a condition, a session branch, and the \
         no-candidate refusal; extra subforms mean an inner form closed early and a branch \
         became unreachable"
    );

    // The no-candidate else branch must BE the typed refusal alist, not an
    // argument to something that signals instead of recording it.
    let refusal = candidate_items.get(3).ok_or("no-candidate branch missing")?;
    let Sexp::List(refusal_rows) = refusal else {
        return Err("no-candidate branch must be a literal refusal alist".into());
    };
    assert!(
        refusal_rows.iter().any(|row| matches!(row.nth_atom(0), Some("refusal_reason"))),
        "the no-candidate branch must itself be the typed refusal record"
    );

    // The connect guard must keep its `error` handler.
    let guard = forms
        .iter()
        .find(|form| form.head() == Some("condition-case") && form.nth_atom(1) == Some("err"))
        .ok_or("connect must be guarded by a named condition-case")?;
    let Sexp::List(guard_items) = guard else {
        return Err("connect guard is not a form".into());
    };
    assert_eq!(
        guard_items.len(),
        4,
        "the connect guard must be `(condition-case err BODY (error ...))` — a guard with no \
         handler cannot record a typed connect refusal"
    );
    assert_eq!(
        guard_items.get(3).and_then(Sexp::head),
        Some("error"),
        "the connect guard's handler must catch `error`"
    );
    Ok(())
}

/// Slot names the driver actually writes into a receipt.
fn driver_receipt_keys(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let forms = driver_forms(root)?;
    let call = forms
        .iter()
        .find(|form| form.head() == Some("perl-lsp-root-probe--record"))
        .ok_or("driver must call its own record writer")?;
    let Sexp::List(items) = call else {
        return Err("record call is not a form".into());
    };
    let payload = items.get(2).ok_or("record call carries no payload alist")?;
    let Sexp::List(rows) = payload else {
        return Err("record payload is not an alist".into());
    };
    rows.iter()
        .map(|row| {
            row.nth_atom(0).map(str::to_owned).ok_or_else(|| "receipt row has no slot name".into())
        })
        .collect()
}

#[test]
fn root_matrix_observation_slots_match_the_driver_receipt_keys() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let record: Value = serde_json::from_str(&read(&root, RECORD)?)?;
    let mut declared: Vec<String> = record
        .pointer("/observation_slots/slots")
        .and_then(Value::as_array)
        .ok_or("observation_slots.slots missing")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    let mut emitted = driver_receipt_keys(&root)?;
    let emitted_count = emitted.len();
    emitted.sort();
    emitted.dedup();
    assert_eq!(emitted.len(), emitted_count, "the driver must not write a slot twice");

    let declared_count = declared.len();
    declared.sort();
    declared.dedup();
    assert_eq!(declared.len(), declared_count, "the declared slot vocabulary must be unique");

    // Closed vocabulary in both directions: an undeclared receipt key and a
    // declared-but-never-written slot are both drift.
    assert_eq!(
        emitted, declared,
        "the driver's receipt keys and the record's declared observation slots must agree exactly"
    );
    Ok(())
}

#[test]
fn root_matrix_instrument_binding_names_every_required_environment() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;
    let record: Value = serde_json::from_str(&read(&root, RECORD)?)?;
    let binding = record
        .pointer("/instrument/binding")
        .and_then(Value::as_str)
        .ok_or("instrument.binding missing")?;

    const PREFIX: &str = "PERL_LSP_EGLOT_ROOT_PROBE_";
    let mut referenced: Vec<&str> = driver
        .match_indices(PREFIX)
        .map(|(start, _)| {
            let tail = &driver[start..];
            let end = tail
                .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .collect();
    referenced.sort_unstable();
    referenced.dedup();
    assert!(!referenced.is_empty(), "driver must read its bindings from the environment");

    // The record has to describe the instrument it actually declares, or a
    // host operator configures a run the driver will refuse.
    for name in referenced {
        let suffix = name.trim_start_matches(PREFIX);
        assert!(
            binding.contains(name) || binding.contains(suffix),
            "instrument.binding must name the required environment {name}"
        );
    }
    Ok(())
}

#[test]
fn root_matrix_observation_denominator_is_explicit_and_unobserved() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let record: Value = serde_json::from_str(&read(&root, RECORD)?)?;
    let cases = record.get("cases").and_then(Value::as_object).ok_or("cases object missing")?;
    let subjects = record
        .get("subjects_in_scope")
        .and_then(Value::as_array)
        .ok_or("subjects_in_scope missing")?
        .len();

    let denominator = record
        .get("observation_denominator")
        .ok_or("the durable denominator must be explicit, not implied by two list lengths")?;
    let read_count = |key: &str| -> Result<u64, Box<dyn Error>> {
        denominator
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("observation_denominator.{key} missing").into())
    };
    assert_eq!(read_count("cases")?, cases.len() as u64, "denominator case count must be real");
    assert_eq!(read_count("subjects_in_scope")?, subjects as u64, "subject count must be real");
    assert_eq!(
        read_count("cells")?,
        (cases.len() * subjects) as u64,
        "the durable denominator is every case x in-scope-subject cell"
    );

    // A cell is one subject's observation of one case: a hosted run may not
    // overwrite another subject's cell, and none is filled yet.
    let mut observed = 0u64;
    for (id, case) in cases {
        let rows = case
            .get("observations")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("case {id} has no observations array"))?;
        assert!(
            rows.len() <= subjects,
            "case {id} may hold at most one observation per in-scope subject"
        );
        let mut seen: Vec<&str> =
            rows.iter().filter_map(|row| row.get("subject_id").and_then(Value::as_str)).collect();
        let filled = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), filled, "case {id} may not record a subject twice");
        assert_eq!(filled, rows.len(), "every observation in case {id} must name its subject");
        observed += rows.len() as u64;
    }
    assert_eq!(read_count("observed_cells")?, observed, "observed cell count must match the rows");
    assert_eq!(observed, 0, "no cell may carry facts before an exact pinned-subject host run");
    Ok(())
}

#[test]
fn root_probe_driver_refuses_an_unreadable_root_uri_instead_of_recording_null()
-> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let driver = read(&root, DRIVER)?;

    // Emacs 29 bundled jsonrpc pretty-prints the outgoing message as a Lisp
    // plist; Emacs 30 logs raw JSON. Reading only one spelling makes every
    // successful session on the other host record a null rootUri that stock
    // never produced.
    assert!(
        driver.contains(":rootUri"),
        "the extractor must read the Emacs 29 plist spelling of rootUri"
    );
    assert!(
        driver.contains("\\\"rootUri\\\""),
        "the extractor must read the Emacs 30 JSON spelling of rootUri"
    );

    // The record's own rule is that missing instrumentation refuses rather
    // than passes, so a log the instrument cannot read may not serialize as
    // an observed null.
    assert!(
        driver.contains("initialize_root_uri_not_extractable"),
        "an unreadable event log must record a typed refusal, not a stock null"
    );

    let forms = driver_forms(&root)?;
    let refusal_is_reachable = forms.iter().any(|form| {
        form.head() == Some("if")
            && form.nth_atom(1) == Some("extracted")
            && matches!(form, Sexp::List(items) if items.len() == 4)
    });
    assert!(
        refusal_is_reachable,
        "the extraction result must select between the observed value and the refusal through a \
         complete `(if extracted OBSERVED REFUSAL)`"
    );
    Ok(())
}
