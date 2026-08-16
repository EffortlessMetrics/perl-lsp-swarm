use crate::model::{FindingKind, TrustedWriter, TrustedWriterPolicy};
use crate::scan::{
    is_known_unconverted, project_root, scan_repository, scan_workflow, scan_workflow_with_policy,
    stale_known_writers,
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
    assert!(findings.iter().any(|finding| finding.kind == FindingKind::CandidateDefinedWriter));
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
    assert!(findings.iter().any(|finding| finding.kind == FindingKind::CandidateDefinedWriter));
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
    assert!(scan_workflow_with_policy("writer.yml", &workflow, &approved_policy()).is_empty());
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
fn pull_request_target_only_writer_runs_the_base_definition() -> Result<(), String> {
    // `pull_request_target` resolves the workflow from the base branch, so editing this
    // file in a candidate does not change what runs. Without a checkout of candidate
    // content the token never meets untrusted bytes, so this is not a candidate writer.
    let workflow = parse(
        r#"
on:
  pull_request_target:
    types: [opened, edited]
permissions:
  pull-requests: write
jobs:
  validate-title:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3
        with:
          script: core.info(context.payload.pull_request.title)
"#,
    )?;
    assert!(scan_workflow("pr-title-check.yml", &workflow).is_empty());
    Ok(())
}

#[test]
fn pull_request_target_writer_checking_out_head_is_still_candidate_defined() -> Result<(), String> {
    // The exemption above must not become a laundering route: once the job checks out the
    // head ref, candidate bytes execute with the write token.
    let workflow = parse(
        r#"
on: pull_request_target
permissions:
  contents: write
jobs:
  build-and-push:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - run: ./scripts/release.sh
"#,
    )?;
    let findings = scan_workflow("laundering.yml", &workflow);
    assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
    assert_eq!(findings[0].kind, FindingKind::CandidateDefinedWriter);
    Ok(())
}

#[test]
fn pull_request_target_paired_with_pull_request_stays_candidate_defined() -> Result<(), String> {
    // A workflow that also triggers on `pull_request` executes the candidate's own copy on
    // that path, so the base-definition exemption must not apply to the whole workflow.
    let workflow = parse(
        r#"
on: [pull_request, pull_request_target]
permissions:
  contents: write
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: ./scripts/publish.sh
"#,
    )?;
    let findings = scan_workflow("mixed.yml", &workflow);
    assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
    assert_eq!(findings[0].kind, FindingKind::CandidateDefinedWriter);
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
    assert!(findings.iter().any(|finding| finding.kind == FindingKind::SelfModifyingWriter));
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
            && finding.detail.contains("candidate-controlled workflow steps")
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

    let new: Vec<_> = findings.iter().filter(|finding| !is_known_unconverted(finding)).collect();
    assert!(
        new.is_empty(),
        "new candidate writer findings (the baseline in KNOWN_UNCONVERTED_WRITERS covers only \
         writers that predate this control; a new one must be built as a trusted writer, not \
         added to the list): {new:#?}"
    );

    // Shrink-only: an entry whose writer is gone must be removed with it, so the baseline
    // cannot silently persist past its subject or be reused to excuse a different job.
    let stale = stale_known_writers(&findings);
    assert!(
        stale.is_empty(),
        "KNOWN_UNCONVERTED_WRITERS entries no longer match any finding — remove them: {stale:?}"
    );

    Ok(())
}
