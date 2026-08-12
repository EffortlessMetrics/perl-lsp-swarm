use crate::model::{FindingKind, TrustedWriter, TrustedWriterPolicy};
use crate::scan::{
    project_root, scan_repository, scan_workflow, scan_workflow_with_policy,
};
use serde_yaml_ng::Value;

fn parse(raw: &str) -> Result<Value, String> {
    serde_yaml_ng::from_str(raw).map_err(|error| format!("test workflow must parse: {error}"))
}

fn approved_policy() -> TrustedWriterPolicy {
    TrustedWriterPolicy::from_writers(
        "candidate-writer.trusted-writers.v1:test",
        [TrustedWriter::new(
            "EffortlessMetrics/repository-controls",
            ".github/workflows/publish.yml",
            "0123456789abcdef0123456789abcdef01234567",
        )],
    )
}

#[test]
fn separate_candidate_prepare_and_publish_jobs_are_rejected() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions:
  contents: read
jobs:
  prepare:
    runs-on: ubuntu-latest
    steps:
      - run: ./scripts/build-patch.sh
  publish:
    permissions:
      contents: write
    runs-on: ubuntu-latest
    steps:
      - run: git add generated.json && git commit -m update && git push
"#,
    )?;
    let findings = scan_workflow("writer.yml", &workflow);
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == FindingKind::CandidateDefinedWriter)
    );
    Ok(())
}

#[test]
fn merge_group_only_writer_is_rejected() -> Result<(), String> {
    let workflow = parse(
        r#"
on: merge_group
permissions:
  contents: write
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: ./opaque-publisher
"#,
    )?;
    let findings = scan_workflow("merge-group.yml", &workflow);
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == FindingKind::CandidateDefinedWriter)
    );
    Ok(())
}

#[test]
fn omitted_permissions_are_not_assumed_read_only() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: ./opaque-publisher
"#,
    )?;
    let findings = scan_workflow("unknown-default.yml", &workflow);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::UnprovenTokenAuthority);
    Ok(())
}

#[test]
fn explicit_read_permissions_are_allowed_for_candidate_producers() -> Result<(), String> {
    let workflow = parse(
        r#"
on: [pull_request, merge_group]
permissions:
  contents: read
jobs:
  prepare:
    runs-on: ubuntu-latest
    steps:
      - run: git diff --binary > candidate.patch
"#,
    )?;
    assert!(scan_workflow("producer.yml", &workflow).is_empty());
    Ok(())
}

#[test]
fn local_reusable_writer_is_rejected() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: ./.github/workflows/publish-generated.yml
"#,
    )?;
    let findings = scan_workflow("writer.yml", &workflow);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::LocalReusableWriter);
    Ok(())
}

#[test]
fn immutable_approved_remote_reusable_writer_is_allowed() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: EffortlessMetrics/repository-controls/.github/workflows/publish.yml@0123456789abcdef0123456789abcdef01234567
"#,
    )?;
    assert!(
        scan_workflow_with_policy("writer.yml", &workflow, &approved_policy()).is_empty()
    );
    Ok(())
}

#[test]
fn immutable_unapproved_remote_is_not_trusted_by_sha_alone() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: attacker/repository/.github/workflows/publish.yml@0123456789abcdef0123456789abcdef01234567
"#,
    )?;
    let findings = scan_workflow_with_policy("writer.yml", &workflow, &approved_policy());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::UntrustedReusableWriter);
    Ok(())
}

#[test]
fn mutable_remote_reusable_writer_is_rejected() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: EffortlessMetrics/repository-controls/.github/workflows/publish.yml@main
"#,
    )?;
    let findings = scan_workflow_with_policy("writer.yml", &workflow, &approved_policy());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::MutableReusableWriter);
    Ok(())
}

#[test]
fn schedule_only_writer_is_outside_candidate_policy() -> Result<(), String> {
    let workflow = parse(
        r#"
on: schedule
permissions:
  contents: write
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: git commit -am update && git push
"#,
    )?;
    assert!(scan_workflow("scheduled.yml", &workflow).is_empty());
    Ok(())
}

#[test]
fn self_deleting_writer_is_rejected_as_its_own_class() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request_target
permissions:
  contents: write
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: git rm .github/workflows/repair.yml && git commit -m cleanup && git push
"#,
    )?;
    let findings = scan_workflow("repair.yml", &workflow);
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == FindingKind::SelfModifyingWriter)
    );
    Ok(())
}

#[test]
fn arbitrary_write_capable_steps_are_rejected_without_command_heuristics() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    runs-on: ubuntu-latest
    steps:
      - run: ./opaque-publisher
"#,
    )?;
    let findings = scan_workflow("writer.yml", &workflow);
    assert!(findings.iter().any(|finding| {
        finding.kind == FindingKind::CandidateDefinedWriter
            && finding
                .detail
                .contains("candidate-controlled workflow steps")
    }));
    Ok(())
}

#[test]
fn schedule_or_always_does_not_exclude_candidate_events() -> Result<(), String> {
    let workflow = parse(
        r#"
on: [pull_request, schedule]
permissions: read-all
jobs:
  publish:
    if: github.event_name == 'schedule' || always()
    permissions:
      contents: write
    runs-on: ubuntu-latest
    steps:
      - run: ./opaque-publisher
"#,
    )?;
    assert!(!scan_workflow("writer.yml", &workflow).is_empty());
    Ok(())
}

#[test]
fn trusted_event_refinement_excludes_candidate_path() -> Result<(), String> {
    let workflow = parse(
        r#"
on: [pull_request, workflow_dispatch]
permissions: read-all
jobs:
  publish:
    if: >-
      github.event_name == 'workflow_dispatch' &&
      github.event.inputs.mode == 'publish'
    permissions:
      contents: write
    runs-on: ubuntu-latest
    steps:
      - run: ./trusted-dispatch-publisher
"#,
    )?;
    assert!(scan_workflow("writer.yml", &workflow).is_empty());
    Ok(())
}

#[test]
fn repository_workflows_satisfy_candidate_writer_policy() -> Result<(), String> {
    let findings = scan_repository(&project_root()?)?;
    assert!(
        findings.is_empty(),
        "candidate writer findings: {findings:#?}"
    );
    Ok(())
}
