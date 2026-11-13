from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one exact match, found {count}")
    file.write_text(text.replace(old, new), encoding="utf-8")


replace_once(
    "scripts/ci/hosted_queue_admission.py",
    '''    current()
    source_current()
    require(re.fullmatch(r"[1-9][0-9]*", env.get("GITHUB_RUN_ID", ""))
''',
    '''    current()
    source_current()
    permission(sender.get("login"))
    permission(result["reviewer"])
    require(re.fullmatch(r"[1-9][0-9]*", env.get("GITHUB_RUN_ID", ""))
''',
)

replace_once(
    "scripts/ci/test_hosted_queue_admission.py",
    '''    def test_rerun_actor_refuses(self):
''',
    '''    def test_authority_revoked_before_receipt_refuses(self):
        for revoked in ['requester','reviewer']:
            with self.subTest(revoked=revoked):
                api=Fake()
                event=copy.deepcopy(self.event)
                event['repository']=api.repo
                event['pull_request']=api.pr
                env=self.env.copy()
                original=api.get
                checks={}
                def get(path):
                    value=original(path)
                    if '/collaborators/' in path:
                        login=path.split('/')[-2]
                        checks[login]=checks.get(login,0)+1
                        if login==revoked and checks[login]>1:
                            value['permission']='read'
                    return value
                api.get=get
                with self.assertRaisesRegex(RuntimeError,'write authority'):
                    q.observe(api,event,env,review=lambda *a:{'reviewer':'reviewer'})
    def test_rerun_actor_refuses(self):
''',
)

replace_once(
    "scripts/tests/test_publication_consistency_hook.py",
    '''        result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
        self.assertNotEqual(result.returncode, 0, calls)


class PublicationReceiptSchemaTests(unittest.TestCase):
''',
    '''        result, calls = self.invoke([(head, "refs/heads/topic", self.base)])
        self.assertNotEqual(result.returncode, 0, calls)
        self.assertEqual(calls, [])
        self.assertIn("NOT PROVEN: Cargo is unavailable", result.stderr)


class PublicationReceiptSchemaTests(unittest.TestCase):
''',
)

replace_once(
    "xtask/src/tasks/gates.rs",
    '''#[cfg(test)]
fn build_agent_receipt(root: &Path, results: &[GateResult], plan: &GatePlan) -> AgentReceipt {
    build_agent_receipt_for_subject(root, results, plan, None)
        .expect("legacy non-publication receipt fixture")
}
''',
    '''#[cfg(test)]
fn build_agent_receipt(
    root: &Path,
    results: &[GateResult],
    plan: &GatePlan,
) -> Result<AgentReceipt> {
    build_agent_receipt_for_subject(root, results, plan, None)
}
''',
)

gates = Path("xtask/src/tasks/gates.rs")
text = gates.read_text(encoding="utf-8")
lines = text.splitlines(keepends=True)
starts = [i for i, line in enumerate(lines) if "let receipt = build_agent_receipt(" in line]
if len(starts) != 3:
    raise SystemExit(
        f"xtask/src/tasks/gates.rs: expected 3 helper call sites, found {len(starts)}"
    )
for start in reversed(starts):
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].strip() == ");"),
        None,
    )
    if end is None:
        raise SystemExit(
            f"xtask/src/tasks/gates.rs: no closing call after line {start + 1}"
        )
    lines[end] = lines[end].replace(");", ")?;")
text = "".join(lines)

replacements = [
    (
        '''        let guidance = empty["suggested_next_actions"][0].as_str().unwrap();
''',
        '''        let guidance = empty["suggested_next_actions"][0]
            .as_str()
            .ok_or_else(|| eyre!("publication guidance missing"))?;
''',
    ),
    (
        '''            let rerun = if failed {
                json["failures"][0]["repro"].as_str().unwrap()
            } else {
                json["advisories"][0]["rerun"].as_str().unwrap()
            };
''',
        '''            let rerun = if failed {
                json["failures"][0]["repro"].as_str()
            } else {
                json["advisories"][0]["rerun"].as_str()
            };
            let rerun = rerun.ok_or_else(|| eyre!("publication rerun missing"))?;
''',
    ),
    (
        '''            let raw = crate::tasks::commit_checks::parse_report(
                result.output_summary.as_deref().unwrap(),
            )
            .unwrap();
''',
        '''            let output_summary = result
                .output_summary
                .as_deref()
                .ok_or_else(|| eyre!("publication gate output missing"))?;
            let raw = crate::tasks::commit_checks::parse_report(output_summary)
                .ok_or_else(|| eyre!("publication gate report missing"))?;
''',
    ),
]
for old, new in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"xtask/src/tasks/gates.rs: expected one receipt-test match, found {count}"
        )
    text = text.replace(old, new)
gates.write_text(text, encoding="utf-8")

replace_once(
    "xtask/src/tasks/gates/publication.rs",
    '''            let deferred = plan
                .skipped
                .iter()
                .find(|row| row.name == gate.name)
                .expect("retained disposition");
''',
    '''            let deferred = plan
                .skipped
                .iter()
                .find(|row| row.name == gate.name)
                .context("retained disposition")?;
''',
)
replace_once(
    "xtask/src/tasks/gates/publication.rs",
    '''        policy.gates.iter_mut().find(|gate| is_mechanical_gate(gate)).unwrap().required = false;
''',
    '''        policy
            .gates
            .iter_mut()
            .find(|gate| is_mechanical_gate(gate))
            .context("mechanical gate present")?
            .required = false;
''',
)

replace_once(
    "xtask/src/tasks/commit_checks.rs",
    '''    let raw = String::from_utf8_lossy(&output.stdout);
    let affected: Vec<String> = raw.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    Ok(CommitCheckOutcome::Flagged(CheckReport {
        check: "whitespace_check".to_string(),
        posture: Posture::Blocked,
        result: format!(
            "`git diff {base} {tree_oid} --check` found {} issue(s) in the staged diff",
            affected.len()
        ),
        why: "trailing whitespace and unresolved merge-conflict markers break formatting and, \\
              for markers, compilation"
            .to_string(),
        affected,
        fix: Some(
            "remove trailing whitespace / resolve the conflict, then re-stage (git add)"
                .to_string(),
        ),
        rerun: rerun_for("whitespace_check"),
        what_remains: "none — this is the full check".to_string(),
    }))
''',
    '''    if output.status.code() != Some(2) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail: String = stderr.trim().chars().take(1024).collect();
        let affected = if detail.is_empty() {
            vec!["git diff --check failed without diagnostic output".to_string()]
        } else {
            vec![detail]
        };
        return Ok(CommitCheckOutcome::Flagged(CheckReport {
            check: "whitespace_check".to_string(),
            posture: Posture::NotProven,
            result: format!(
                "`git diff {base} {tree_oid} --check` could not establish whitespace status \\
                 (exit {:?})",
                output.status.code()
            ),
            why: "the Git comparison or one of its objects was unavailable; treating an \\
                  instrument failure as a clean or source-blocked candidate would invent a verdict"
                .to_string(),
            affected,
            fix: Some(
                "make both compared Git objects available, then rerun the exact whitespace check"
                    .to_string(),
            ),
            rerun: rerun_for("whitespace_check"),
            what_remains: "whitespace status remains NOT PROVEN until the Git comparison executes"
                .to_string(),
        }));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let affected: Vec<String> = raw.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    Ok(CommitCheckOutcome::Flagged(CheckReport {
        check: "whitespace_check".to_string(),
        posture: Posture::Blocked,
        result: format!(
            "`git diff {base} {tree_oid} --check` found {} issue(s) in the staged diff",
            affected.len()
        ),
        why: "trailing whitespace and unresolved merge-conflict markers break formatting and, \\
              for markers, compilation"
            .to_string(),
        affected,
        fix: Some(
            "remove trailing whitespace / resolve the conflict, then re-stage (git add)"
                .to_string(),
        ),
        rerun: rerun_for("whitespace_check"),
        what_remains: "none — this is the full check".to_string(),
    }))
''',
)

replace_once(
    "xtask/src/tasks/commit_checks.rs",
    '''    #[test]
    fn posture_is_blocking_only_for_blocked() -> Result<()> {
''',
    '''    #[test]
    fn whitespace_check_unknown_object_is_not_proven_not_blocked() -> Result<()> {
        let repo = TempRepo::init()?;
        repo.write("foo.rs", "fn main() {}\\n")?;
        repo.add("foo.rs")?;
        repo.commit("initial")?;
        let missing = "f".repeat(40);

        match whitespace_check_against(repo.root(), Some(&missing), None)? {
            CommitCheckOutcome::Flagged(report) => {
                assert_eq!(report.check, "whitespace_check");
                assert_eq!(
                    report.posture,
                    Posture::NotProven,
                    "an unavailable object is an instrument failure, not a source finding: {report:?}"
                );
                assert!(
                    !report.affected.is_empty(),
                    "instrument failure must retain bounded Git diagnostics"
                );
            }
            CommitCheckOutcome::Pass(summary) => {
                bail!("unknown object must not read as a clean whitespace result: {summary}")
            }
        }
        Ok(())
    }

    #[test]
    fn posture_is_blocking_only_for_blocked() -> Result<()> {
''',
)
