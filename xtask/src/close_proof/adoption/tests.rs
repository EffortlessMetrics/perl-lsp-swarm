use super::source::{MAX_RESPONSE_BYTES, READ_COUNT, SourceReader, observe};
use super::*;
use serde_json::{Value, json};

fn sources() -> SimulationSources {
    SimulationSources {
        comment_first: include_bytes!("fixtures/15627/comment.json").to_vec(),
        issue_first: include_bytes!("fixtures/15627/issue.json").to_vec(),
        policy_commit: include_bytes!("fixtures/15627/policy.json").to_vec(),
        template_metadata: include_bytes!("fixtures/15627/template-metadata.json").to_vec(),
        template_bytes: include_bytes!("fixtures/15627/template.md").to_vec(),
        issue_last: include_bytes!("fixtures/15627/issue.json").to_vec(),
        comment_last: include_bytes!("fixtures/15627/comment.json").to_vec(),
    }
}
fn candidate() -> Result<CandidateMapping, String> {
    let s = sources();
    let selection = CallerExpectedSelection::root_selected_15627();
    let mut reader = Reader::new(s);
    compile_mapping(&selection, &observe(&mut reader, &selection, false)?)
}
struct Reader {
    responses: Vec<Vec<u8>>,
    calls: Vec<(String, bool)>,
    fail: Option<usize>,
}
impl Reader {
    fn new(s: SimulationSources) -> Self {
        Self {
            responses: vec![
                s.comment_first,
                s.issue_first,
                s.policy_commit,
                s.template_metadata,
                s.template_bytes,
                s.issue_last,
                s.comment_last,
            ],
            calls: Vec::new(),
            fail: None,
        }
    }
}
impl SourceReader for Reader {
    fn read(&mut self, endpoint: &str, raw: bool) -> Result<Vec<u8>, String> {
        let index = self.calls.len();
        self.calls.push((endpoint.into(), raw));
        if self.fail == Some(index) {
            return Err("simulated denied/network/timeout/incomplete read".into());
        }
        self.responses.get(index).cloned().ok_or_else(|| "incomplete source read".into())
    }
}
fn edited(raw: &[u8], f: impl FnOnce(&mut Value)) -> Result<Vec<u8>, String> {
    let mut v: Value = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
    f(&mut v);
    serde_json::to_vec(&v).map_err(|e| e.to_string())
}
fn resign(candidate: &mut CandidateMapping) -> Result<(), String> {
    candidate.mapping_digest = candidate.compute_digest().map_err(|e| e.to_string())?;
    Ok(())
}
fn pairs(cases: &[(&str, fn(&mut CandidateMapping))]) -> Result<(), String> {
    let baseline = candidate()?;
    let selection = CallerExpectedSelection::root_selected_15627();
    let s = sources();
    let mut missed = Vec::new();
    for (name, mutate) in cases {
        let mut changed = baseline.clone();
        mutate(&mut changed);
        resign(&mut changed)?;
        let report = report_simulation(&selection, &changed, &s);
        let refused = report.status == ReportStatus::NotProven;
        println!("adoption_mutation name={name} independently_refused={refused}");
        if !refused {
            missed.push(*name);
        }
        assert_eq!(
            report_simulation(&selection, &baseline, &s).status,
            ReportStatus::Simulation,
            "restored selected pilot must still match"
        );
    }
    assert!(missed.is_empty(), "self-consistent candidate escaped fixed selection: {missed:?}");
    Ok(())
}

#[test]
fn red_adoption_candidate_bindings_cannot_replace_fixed_selection() -> Result<(), String> {
    pairs(&[
        ("candidate-repository", |c| c.binding.repository = "attacker/other".into()),
        ("candidate-issue", |c| c.binding.issue = 15628),
        ("candidate-comment-id", |c| c.binding.comment_id = 6001911472),
        ("candidate-record-digest", |c| c.binding.record_digest = "a".repeat(64)),
        ("candidate-issue-digest", |c| c.binding.issue_digest = "b".repeat(64)),
        ("candidate-policy-generation", |c| c.binding.policy_commit = "c".repeat(40)),
        ("candidate-template-blob", |c| c.binding.template_blob = "d".repeat(40)),
        ("candidate-template-digest", |c| c.binding.template_digest = "e".repeat(64)),
    ])
}

#[test]
fn red_adoption_rows_controls_and_proof_cannot_be_weakened() -> Result<(), String> {
    pairs(&[
        ("top-proof-weakened", |c| c.material.required_proof_level = ProofLevel::Representation),
        ("row-proof-weakened", |c| {
            c.material.rows[0].required_proof_level = ProofLevel::Representation
        }),
        ("row-statement-weakened", |c| {
            c.material.rows[0].statement = "Counts are sufficient.".into()
        }),
        ("row-stable-id-replaced", |c| c.material.rows[2].row_id = "replacement.same-count".into()),
        ("row-a3-removed", |c| {
            c.material.rows.remove(2);
        }),
        ("row-added", |c| {
            let mut r = c.material.rows[2].clone();
            r.row_id = "extra.row".into();
            c.material.rows.push(r);
        }),
        ("a2-guard-a1-removed", |c| {
            c.material.negative_controls.remove(0);
        }),
        ("a2-guard-a4-removed", |c| {
            c.material.negative_controls.remove(1);
        }),
        ("a2-guard-retargeted", |c| {
            c.material.negative_controls[0].guards_row_id = c.material.rows[2].row_id.clone()
        }),
        ("a2-opposing-description-weakened", |c| {
            c.material.negative_controls[0].description = "No opposition needed.".into()
        }),
        ("source-quote-weakened", |c| {
            c.material.source_acceptance_quotes[0] = "Any count matches.".into()
        }),
    ])
}

#[test]
fn red_adoption_template_lexical_review_and_non_goal_survive() -> Result<(), String> {
    pairs(&[
        ("template-approximation", |c| c.material.template.digest = "f".repeat(64)),
        ("template-source-identity", |c| c.material.template.identity = "handmade/template".into()),
        ("literal-released-dropped", |c| c.material.lexical_inputs.retain(|x| x != "released")),
        ("literal-release-dropped", |c| c.material.lexical_inputs.retain(|x| x != "release")),
        ("literal-lexical-disposition-weakened", |c| {
            c.material.lexical_disposition = "release proves every related word.".into()
        }),
        ("non-goal15621-lost", |c| c.material.retained_non_goals_and_context.clear()),
        ("declared-review-identity-replaced", |c| {
            c.material.declared_review_basis.identity = "candidate-accepted".into()
        }),
        ("declared-review-digest-recomputed", |c| {
            c.material.declared_review_basis.digest = "0".repeat(64)
        }),
        ("declared-review-text-lost", |c| c.material.declared_review_text.clear()),
        ("child-invented", |c| {
            c.material.mandatory_children.push(super::super::IssueRef {
                repository: c.binding.repository.clone(),
                number: 15621,
            })
        }),
        ("transfer-invented", |c| c.material.permitted_transferred_rows.push(ROW_IDS[0].into())),
        ("nonterminal-limitation-lost", |c| {
            c.material.limitations.pop();
        }),
        ("self-granted-terminal-limitation", |c| {
            c.material.limitations.push("Candidate acceptance authorizes completion.".into())
        }),
    ])
}

#[test]
fn control_adoption_exact_mapping_permutations_and_simulation_ceiling() -> Result<(), String> {
    let selection = CallerExpectedSelection::root_selected_15627();
    let s = sources();
    let mut c = candidate()?;
    let original = c.mapping_digest.clone();
    // Independent literal oracle copied from selected native sources, not compile_mapping.
    let expected_rows = [
        (
            "cp00.pr-asserted-proof-not-excluded",
            "affirmative coverage of the named surface is not treated as exclusion merely because unrelated exclusion language occurs elsewhere. This is a CP00 detector property, not evidence that the underlying product claim is true.",
        ),
        (
            "cp00.pr-genuine-exclusion-detected",
            "with the corresponding issue requirement fixed and present, a genuine PR disclaimer still triggers the proof-level contradiction rule. A2 remains an acceptance row in its own right and a mandatory negative control guarding A1 and A4. Disabling the detector cannot satisfy it.",
        ),
        (
            "cp00.pr-side-variation-isolated",
            "proof varies PR-side text while retaining the issue requirement; a test that passes because it removed or changed that requirement does not establish this row.",
        ),
        (
            "cp00.template-vocabulary-inert",
            "normal template/Claim Boundary vocabulary alone does not create an exclusion. The actual pinned template above must drive this test; a hand-authored approximation alone does not establish A4.",
        ),
    ];
    assert_eq!(c.material.rows.len(), expected_rows.len());
    for (row, (id, statement)) in c.material.rows.iter().zip(expected_rows) {
        assert_eq!(row.row_id, id);
        assert_eq!(row.statement, statement);
        assert_eq!(row.required_proof_level, ProofLevel::Mechanism);
    }
    assert_eq!(c.material.required_proof_level, ProofLevel::Mechanism);
    assert_eq!(c.material.source_acceptance_quotes, vec![
        "A Claim Boundary that *asserts* a public/installed/packaged/released surface is covered does not arm CP00.".to_string(),
        "A Claim Boundary that genuinely disclaims one still arms it (negative control).".to_string(),
        "A test varies the **PR** body with the issue fixed, mirroring the existing issue-side polarity tests.".to_string(),
        "A PR filling in `.github/PULL_REQUEST_TEMPLATE.md`'s Claim Boundary as written does not arm the rule merely by using the template's vocabulary.".to_string(),
    ]);
    assert_eq!(c.material.negative_controls.len(), 2);
    for (guard, (id, target)) in c.material.negative_controls.iter().zip([
        ("cp00.a2-opposing-control.1", "cp00.pr-asserted-proof-not-excluded"),
        ("cp00.a2-opposing-control.2", "cp00.template-vocabulary-inert"),
    ]) {
        assert_eq!(guard.control_id, id);
        assert_eq!(guard.guards_row_id, target);
        assert_eq!(guard.description, expected_rows[1].1);
    }
    assert_eq!(
        c.material.lexical_inputs,
        vec![
            "installed",
            "public",
            "packaged",
            "presentation",
            "release",
            "released",
            "actual host"
        ]
    );
    assert_eq!(
        c.material.lexical_disposition,
        "The original word **released** remains in acceptance. The issue body's six-term detector context—installed, public, packaged, presentation, release, actual host—does not remove or replace it.\n\nInclude literal `released` and the detector's `release` as separately identified paired test inputs where applicable. A passing literal `release` case is not proof of literal `released` behavior. No general stemming/morphology guarantee is added. Untested literal-form behavior remains **NOT_PROVEN**, not excluded and not complete. Coverage must not silently narrow to public-only examples."
    );
    assert_eq!(
        c.material.retained_non_goals_and_context,
        "- Preserve the explicit non-goal: this issue does not decide whether #15621 should use Closes or Advances for #7129. This adoption does not independently endorse the incidental assertion that #15621's acceptance is met.\n- The corroborating Markdown-table report is regression input for these rows, not a fifth acceptance row.\n- The suggested helper implementation is latitude, not a mandatory architecture. No universal natural-language classifier is claimed.\n- No mandatory child or permitted transfer is declared. Related issue/PR references are context, not inherited obligations or transfer authority.\n- Earlier research/repair comments do not become adopted rulings merely through recency, authorship or issue state."
    );
    assert_eq!(
        c.material.declared_review_text,
        "Independent LLM challenge checked the four source Acceptance rows, mechanism proof boundary, child/transfer inference, negative controls and actual-template requirement. It found the mapping faithful with an explicit lexical limitation.\n\nThe root resolves the lexical question by retaining literal `released` in acceptance alongside separately identified `release` tests, rather than using bounded vocabulary to drop it. A2 remains both an acceptance row and mandatory opposing control; the actual pinned template remains load-bearing. No test execution or semantic completion is claimed by this review."
    );
    assert_eq!(
        c.material.declared_review_basis.identity,
        "issuecomment:6001911471/declared-review"
    );
    assert_eq!(
        c.material.declared_review_basis.digest,
        "df0b5cb84f024d62933f682da558919696593e564c0213aa14868702e6860069"
    );
    assert_eq!(
        c.material.template.identity,
        "6b774b54811572291a657067a6e1b4f9072f9ebe:.github/PULL_REQUEST_TEMPLATE.md"
    );
    assert_eq!(
        c.material.template.digest,
        "2b0e27b17a174164bf6c3eba0fa42e80792f2d315a60c961f8195d8b404e4215"
    );
    assert!(c.material.mandatory_children.is_empty());
    assert!(c.material.permitted_transferred_rows.is_empty());
    assert_eq!(
        c.material.limitations,
        vec![
            "Partial mapping; no inferred issue kind or close modes.",
            "Declared review basis; caller independently resolves role authority and ruling conflicts.",
            "Untested literal forms are NOT_PROVEN, not excluded or complete.",
            "No producer admission, semantic completion or issue-close authorization.",
        ]
    );
    assert_eq!(report_live(&selection, &c).status, ReportStatus::NotProven);
    c.material.rows.reverse();
    c.material.negative_controls.reverse();
    c.material.lexical_inputs.reverse();
    c.material.source_acceptance_quotes.reverse();
    c.material.limitations.reverse();
    resign(&mut c)?;
    assert_eq!(original, c.mapping_digest);
    let report = report_simulation(&selection, &c, &s);
    assert_eq!(report.status, ReportStatus::Simulation);
    assert!(report.mapping.is_some());
    assert_eq!(report.semantic_completion, "not_evaluated");
    assert_eq!(report.evidence_admission, "not_evaluated");
    assert!(!report.issue_close_authorized);
    assert!(report.caller_precondition.contains("does not authenticate"));
    assert!(report.observation_limit.contains("ABA"));
    Ok(())
}

#[test]
fn control_adoption_independent_source_identity_and_exact_bytes_refuse() -> Result<(), String> {
    let selection = CallerExpectedSelection::root_selected_15627();
    let c = candidate()?;
    let original = sources();
    let mut cases: Vec<(&str, SimulationSources)> = Vec::new();
    for (name, key, value) in [
        ("observed-comment-id", "id", json!(6001911472u64)),
        (
            "observed-wrong-issue-url",
            "issue_url",
            json!("https://api.github.com/repos/EffortlessMetrics/perl-lsp-swarm/issues/15628"),
        ),
        (
            "observed-wrong-repository",
            "url",
            json!("https://api.github.com/repos/attacker/other/issues/comments/6001911471"),
        ),
    ] {
        let mut s = original.clone();
        s.comment_first = edited(&s.comment_first, |v| v[key] = value)?;
        cases.push((name, s));
    }
    for (name, edit) in [
        ("body-trailing-newline", 0),
        ("body-leading-whitespace", 1),
        ("body-crlf-normalization", 2),
        ("body-missing-declared-review", 3),
    ] {
        let mut s = original.clone();
        s.comment_first = edited(&s.comment_first, |v| {
            let body = v["body"].as_str().unwrap_or("");
            v["body"] = json!(match edit {
                0 => format!("{body}\n"),
                1 => format!(" {body}"),
                2 => body.replace('\n', "\r\n"),
                _ => body.replace("Independent LLM challenge", "candidate alone"),
            });
        })?;
        cases.push((name, s));
    }
    let mut s = original.clone();
    s.issue_first = edited(&s.issue_first, |v| v["number"] = json!(15628))?;
    cases.push(("observed-wrong-issue", s));
    let mut s = original.clone();
    s.issue_first = edited(&s.issue_first, |v| v["pull_request"] = json!({"url":"pretend"}))?;
    cases.push(("observed-pr-not-issue", s));
    let mut s = original.clone();
    s.issue_first = edited(&s.issue_first, |v| v["body"] = json!("weakened current issue"))?;
    cases.push(("observed-edited-issue", s));
    let mut s = original.clone();
    s.policy_commit = br#"{"sha":"another-generation"}"#.to_vec();
    cases.push(("observed-policy-generation", s));
    let mut s = original.clone();
    s.template_metadata = edited(&s.template_metadata, |v| v["sha"] = json!("another-blob"))?;
    cases.push(("observed-template-blob", s));
    let mut s = original.clone();
    s.template_bytes.extend_from_slice(b"approximation");
    cases.push(("observed-template-approximation", s));
    let mut s = original.clone();
    s.comment_last = edited(&s.comment_last, |v| v["body"] = json!("edited between reads"))?;
    cases.push(("observed-stale-record-join", s));
    let mut s = original.clone();
    s.issue_last.clear();
    cases.push(("observed-incomplete-join", s));
    for (name, s) in cases {
        assert_eq!(report_simulation(&selection, &c, &s).status, ReportStatus::NotProven, "{name}");
    }
    assert_eq!(report_simulation(&selection, &c, &original).status, ReportStatus::Simulation);
    Ok(())
}

#[test]
fn control_adoption_transport_budget_failures_and_fixed_get_selection() -> Result<(), String> {
    let selection = CallerExpectedSelection::root_selected_15627();
    for index in 0..READ_COUNT {
        let mut r = Reader::new(sources());
        r.fail = Some(index);
        assert!(observe(&mut r, &selection, false).is_err());
        assert_eq!(r.calls.len(), index + 1);
    }
    let mut r = Reader::new(sources());
    let observed = observe(&mut r, &selection, false)?;
    assert!(!observed.native);
    assert_eq!(r.calls.len(), READ_COUNT);
    assert!(r.calls[0].0.ends_with("/issues/comments/6001911471"));
    assert!(r.calls[1].0.ends_with("/issues/15627"));
    assert!(r.calls[3].0.ends_with("?ref=6b774b54811572291a657067a6e1b4f9072f9ebe"));
    assert!(r.calls[4].1);
    assert!(r.calls.iter().all(|(endpoint, _)| !endpoint.ends_with("/comments")));
    for raw in [
        vec![b'x'; MAX_RESPONSE_BYTES + 1],
        vec![0xff],
        br#"{"id":1,"id":2}"#.to_vec(),
        b"[]".to_vec(),
    ] {
        let mut s = sources();
        s.comment_first = raw;
        let mut r = Reader::new(s);
        assert!(observe(&mut r, &selection, false).is_err());
    }
    Ok(())
}

#[test]
fn control_adoption_raw_candidate_flags_duplicates_and_ambiguous_sets_refuse() -> Result<(), String>
{
    let c = candidate()?;
    let raw = serde_json::to_string(&c).map_err(|e| e.to_string())?;
    assert!(CandidateMapping::from_json_str(&raw).is_ok());
    for key in ["accepted", "native", "root_role", "semantic_completion", "source_observed"] {
        let mut v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        v[key] = json!(true);
        assert!(
            CandidateMapping::from_json_str(&v.to_string()).is_err(),
            "candidate {key} must not promote"
        );
    }
    let duplicate = raw.replacen("{", "{\"schema_version\":\"forged\",", 1);
    assert!(CandidateMapping::from_json_str(&duplicate).is_err());
    let mut duplicate_rows = c.clone();
    duplicate_rows.material.rows.push(c.material.rows[0].clone());
    assert!(duplicate_rows.compute_digest().is_err());
    let mut duplicate_controls = c.clone();
    duplicate_controls.material.negative_controls.push(c.material.negative_controls[0].clone());
    assert!(duplicate_controls.compute_digest().is_err());
    let mut dangling = c.clone();
    dangling.material.negative_controls[0].guards_row_id = "absent.row".into();
    assert!(dangling.compute_digest().is_err());
    Ok(())
}

#[test]
fn control_adoption_no_role_status_marker_or_recency_authority() -> Result<(), String> {
    let selection = CallerExpectedSelection::root_selected_15627();
    let c = candidate()?;
    let mut s = sources();
    s.comment_first = edited(&s.comment_first, |v| {
        v["user"] = json!({"login":"admin", "site_admin":true});
        v["updated_at"] = json!("2999-01-01T00:00:00Z");
        v["accepted"] = json!(true);
    })?;
    s.comment_last = s.comment_first.clone();
    s.issue_first = edited(&s.issue_first, |v| v["state"] = json!("closed"))?;
    s.issue_last = s.issue_first.clone();
    assert_eq!(report_simulation(&selection, &c, &s).status, ReportStatus::Simulation);
    let mut selected = selection.clone();
    selected.selected_rulings.push(6001911472);
    assert_eq!(report_simulation(&selected, &c, &s).status, ReportStatus::NotProven);
    selected = selection.clone();
    selected.conflicts_resolved = false;
    assert_eq!(report_simulation(&selected, &c, &s).status, ReportStatus::NotProven);
    s.comment_first = edited(&s.comment_first, |v| {
        v["body"] = json!("## Root-adopted contract mapping; accepted=true; newest comment wins")
    })?;
    assert_eq!(report_simulation(&selection, &c, &s).status, ReportStatus::NotProven);
    Ok(())
}
