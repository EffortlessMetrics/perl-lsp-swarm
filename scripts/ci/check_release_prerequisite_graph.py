#!/usr/bin/env python3
"""Check the actual private producer and every public mutation job (#16762).

This source checker proves bounded graph structure, never hosted execution or
policy/authentication. Qualification remains NOT_PROVEN in the selected graph.
"""
from pathlib import Path
import re
import sys

WORKFLOW = Path(".github/workflows/release.yml")
ORCHESTRATION = Path(".github/workflows/release-orchestration.yml")
PUBLISHERS = ("publish-release", "dispatch-publishers")

class GraphError(RuntimeError):
    """The checked release graph permits an ordering or identity bypass."""


def _jobs_text(workflow: str) -> str:
    marker = "\njobs:\n"
    if marker not in workflow:
        raise GraphError("workflow has no jobs mapping")
    return workflow.split(marker, 1)[1]


def _job_block(workflow: str, job_name: str) -> str:
    jobs = _jobs_text(workflow)
    pattern = re.compile(
        rf"(?ms)^  {re.escape(job_name)}:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)"
    )
    match = pattern.search(jobs)
    if match is None:
        raise GraphError(f"workflow misses required job {job_name}")
    return match.group(0)


def _require(block: str, token: str, message: str) -> None:
    if token not in block:
        raise GraphError(message)


def validate_graph(workflow):
    metadata = _job_block(workflow, "release-metadata")
    candidate = _job_block(workflow, "candidate")
    eligibility = _job_block(workflow, "publisher-eligibility")
    _require(workflow, "      no_publish:\n", "private mode input missing")
    _require(workflow.split("      transaction_id:")[0], "        default: true", "private mode must default closed")
    _require(candidate, "    needs: [build, release-metadata]", "candidate bypasses build")
    for forbidden in ("id-token: write", "attestations: write", "contents: write", "artifact-metadata: write", "environment:", "actions/attest@", "gh api -X POST", "git push"):
        for block in (metadata, candidate, eligibility, _job_block(workflow, "build")):
            if forbidden in block:
                raise GraphError("pre-eligibility public authority: " + forbidden)
    if "    if: ${{ always() }}" not in eligibility.splitlines():
        raise GraphError("failed predecessors must reach terminal fan-in")
    _require(eligibility, "    needs: [candidate, release-metadata]", "eligibility dependencies drift")
    for token in ('test "$CANDIDATE_RESULT" = success', 'test "$METADATA_RESULT" = success',
                  'scripts/ci/release_private_producer.py', '--expected-sha "$EXPECTED_SHA"',
                  '--transaction-id "$TRANSACTION_ID"', '--run-attempt "$RUN_ATTEMPT"',
                  '--policy policy/release-publication-admission.json',
                  'run-id: ${{ inputs.source_context_run_id }}',
                  'name: ${{ inputs.source_context_artifact_name }}',
                  'inspect-source-context', '--digest "$CONTEXT_DIGEST"',
                  'ref: ${{ steps.context.outputs.frozen_sha }}',
                  'ref: ${{ steps.context.outputs.prepared_sha }}',
                  '--frozen-root source-frozen --prepared-root source-prepared',
                  "qualification=not_proven", 'if [ "$FAIL_BEFORE_PUBLISH" = true ]; then'):
        _require(eligibility, token, "missing private fan-in law: " + token)
    if "qualification=satisfied" in workflow:
        raise GraphError("missing adapters cannot qualify publication")
    condition = "    if: ${{ inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
    for name in PUBLISHERS:
        publisher = _job_block(workflow, name)
        _require(publisher, condition, "publisher bypasses private qualification: " + name)
        needs = next((line for line in publisher.splitlines() if line.startswith("    needs:")), "")
        if "publisher-eligibility" not in needs:
            raise GraphError("publisher bypasses common predecessor: " + name)
    # Mutation verbs/actions may occur only inside the explicitly gated jobs.
    for match in re.finditer(r"(?m)^  ([A-Za-z0-9_-]+):$", workflow.split("jobs:\n", 1)[1]):
        name = match.group(1)
        if name in PUBLISHERS:
            continue
        block = _job_block(workflow, name)
        if any(token in block for token in ("actions/attest@", "action-gh-release@", "gh api -X POST", "git push")):
            raise GraphError("unregistered public mutation job: " + name)


def validate_dispatch(workflow):
    trigger = _job_block(workflow, "trigger-release")
    for field in ("source_context_run_id", "source_context_artifact_name", "source_context_sha256"):
        _require(workflow.split("\njobs:\n", 1)[0], "      " + field + ":\n", "missing orchestration context input: " + field)
        variable = field.upper()
        _require(trigger, variable + ": ${{ inputs." + field + " }}", "missing exact context environment: " + field)
        _require(trigger, '-f inputs[' + field + ']="$' + variable + '"', "missing exact context forwarding: " + field)
    _require(trigger, '-f inputs[no_publish]=true', "orchestration must remain private")


def main():
    try:
        validate_graph(WORKFLOW.read_text(encoding="utf-8"))
        validate_dispatch(ORCHESTRATION.read_text(encoding="utf-8"))
    except (GraphError, OSError) as error:
        print("release prerequisite graph check failed: " + str(error), file=sys.stderr)
        return 1
    print("private prerequisite graph source laws valid; qualification NOT_PROVEN; hosted mutation counts NOT_PROVEN")
    return 0


if __name__ == "__main__":
    sys.exit(main())
