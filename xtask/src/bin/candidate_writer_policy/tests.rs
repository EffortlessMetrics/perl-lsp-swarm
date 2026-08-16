use crate::model::{FindingKind, TrustedWriter, TrustedWriterPolicy, partition_incidents};
use crate::scan::{project_root, scan_repository, scan_workflow, scan_workflow_with_policy};
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
fn repository_has_no_new_candidate_defined_writers() -> Result<(), String> {
    let findings = scan_repository(&project_root()?)?;
    let partition = partition_incidents(&findings);
    assert!(
        partition.new_findings.is_empty(),
        "new candidate writer findings: {:#?}",
        partition.new_findings
    );
    Ok(())
}

/// The recorded-incident list is a ratchet, so it may only shrink.
#[test]
fn known_incidents_still_reproduce() -> Result<(), String> {
    let findings = scan_repository(&project_root()?)?;
    let partition = partition_incidents(&findings);
    assert!(
        partition.stale.is_empty(),
        "recorded incidents no longer reproduce and must be removed: {:#?}",
        partition.stale
    );
    Ok(())
}

#[test]
fn pull_request_target_steps_are_not_candidate_defined() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request_target
permissions:
  pull-requests: write
jobs:
  validate:
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
fn pull_request_target_checking_out_candidate_code_is_rejected() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request_target
permissions:
  contents: write
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - run: ./scripts/build.sh
"#,
    )?;
    let findings = scan_workflow("target-writer.yml", &workflow);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::CandidateCodeExecutionWriter);
    Ok(())
}

/// A base-sourced write-capable job that checks out the base tree is safe.
#[test]
fn pull_request_target_checking_out_base_is_allowed() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request_target
permissions:
  contents: write
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - run: ./scripts/build.sh
"#,
    )?;
    assert!(scan_workflow("target-base.yml", &workflow).is_empty());
    Ok(())
}

/// A workflow carrying both sourcings keeps the stronger candidate-sourced rule.
#[test]
fn mixed_sourcing_keeps_candidate_defined_rule() -> Result<(), String> {
    let workflow = parse(
        r#"
on: [pull_request, pull_request_target]
permissions:
  contents: write
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: ./opaque-publisher
"#,
    )?;
    let findings = scan_workflow("mixed.yml", &workflow);
    assert!(findings.iter().any(|finding| finding.kind == FindingKind::CandidateDefinedWriter));
    Ok(())
}

/// Trust is the exact triple, not the repository alone.
#[test]
fn approved_repository_at_a_different_workflow_path_is_not_trusted() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: EffortlessMetrics/repository-controls/.github/workflows/other.yml@0123456789abcdef0123456789abcdef01234567
"#,
    )?;
    let findings = scan_workflow_with_policy("writer.yml", &workflow, &approved_policy());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::UntrustedReusableWriter);
    Ok(())
}

/// Trust is the exact triple, not the repository and path at any revision.
#[test]
fn approved_workflow_at_a_different_commit_is_not_trusted() -> Result<(), String> {
    let workflow = parse(
        r#"
on: pull_request
permissions: read-all
jobs:
  publish:
    permissions:
      contents: write
    uses: EffortlessMetrics/repository-controls/.github/workflows/publish.yml@fedcba9876543210fedcba9876543210fedcba98
"#,
    )?;
    let findings = scan_workflow_with_policy("writer.yml", &workflow, &approved_policy());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, FindingKind::UntrustedReusableWriter);
    Ok(())
}
