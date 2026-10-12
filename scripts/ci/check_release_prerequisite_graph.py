#!/usr/bin/env python3
"""Restricted source grammar for the selected private release graph (#16762).

Only the current plain-key, inline-needs, named-step and literal-run style is
admitted. Unsupported/duplicate authority fields fail closed. This is not a
YAML or shell interpreter, nor hosted execution/authentication evidence.
"""
from pathlib import Path
import re
import hashlib
import shlex
import sys

WORKFLOW = Path(".github/workflows/release.yml")
ORCHESTRATION = Path(".github/workflows/release-orchestration.yml")
PUBLISHERS = ("publish-release", "dispatch-publishers")


class GraphError(RuntimeError):
    """The selected source cannot establish the required private graph laws."""


def _uncomment(line):
    quote = None
    escaped = False
    for index, character in enumerate(line):
        if escaped:
            escaped = False
        elif character == "\\" and quote == '"':
            escaped = True
        elif character in ("'", '"'):
            quote = None if quote == character else character if quote is None else quote
        elif character == "#" and quote is None and (index == 0 or line[index - 1].isspace()):
            return line[:index].rstrip()
    return line.rstrip()


def _plain_fields(block, indent):
    """Only plain unique keys are supported at this mapping boundary."""
    fields = {}
    for line in block.splitlines():
        live = _uncomment(line)
        if not live.strip() or len(live) - len(live.lstrip()) != indent:
            continue
        match = re.fullmatch(r"[ ]*([A-Za-z_][A-Za-z0-9_-]*):(?:[ ]*(.*))?", live)
        if match is None or match.group(1) in fields:
            raise GraphError("unsupported/duplicate mapping key")
        fields[match.group(1)] = (match.group(2) or "").strip()
    return fields


def _field(block, key, indent, *, mapping=False):
    _plain_fields(block, indent)
    lines = block.splitlines()
    prefix = " " * indent + key + ":"
    matches = [(i, _uncomment(line)[len(prefix):].strip()) for i, line in enumerate(lines)
               if _uncomment(line).startswith(prefix)]
    if len(matches) != 1:
        raise GraphError("missing/duplicate supported field: " + key)
    index, value = matches[0]
    if mapping:
        if value:
            raise GraphError("unsupported mapping style: " + key)
        end = index + 1
        while end < len(lines):
            line = lines[end]
            if line.strip() and not line.lstrip().startswith("#") and len(line) - len(line.lstrip()) <= indent:
                break
            end += 1
        return "\n".join(lines[index + 1:end])
    if not value or value.startswith(('"', "'", "&", "*", "|", ">", "{", "!")):
        raise GraphError("unsupported scalar style: " + key)
    return value


def _jobs_text(workflow):
    if "\t" in workflow or re.search(r"(?m)^\s*(?:---|\.\.\.)\s*$", workflow):
        raise GraphError("unsupported tabs/multiple-document style")
    jobs = _field(workflow, "jobs", 0, mapping=True)
    names = []
    for line in jobs.splitlines():
        live = _uncomment(line)
        if not live.strip():
            continue
        if len(live) - len(live.lstrip()) == 2:
            match = re.fullmatch(r"  ([A-Za-z0-9_-]+):", live)
            if match is None or match.group(1) in names:
                raise GraphError("unsupported/duplicate plain job key")
            names.append(match.group(1))
        scalar = re.match(r"^\s*[A-Za-z_][A-Za-z0-9_-]*:\s*([\"'])(.*)$", live)
        if scalar and (not scalar.group(2) or not scalar.group(2).endswith(scalar.group(1))):
            raise GraphError("unsupported multiline quoted YAML scalar")
        if re.match(r"^\s*(?:<<:|[A-Za-z_][A-Za-z0-9_-]*:\s*[&*]\S)", live):
            raise GraphError("unsupported YAML alias/merge authority")
    return jobs


def _job_block(workflow, name):
    return _field(_jobs_text(workflow), name, 2, mapping=True)


def _needs(job, expected):
    value = _field(job, "needs", 4)
    if not re.fullmatch(r"\[[A-Za-z0-9_, -]+\]", value):
        raise GraphError("unsupported dependency style")
    members = [part.strip() for part in value[1:-1].split(",")]
    if len(members) != len(set(members)) or set(members) != set(expected):
        raise GraphError("actual dependency membership differs")


def _step(job, name):
    steps = _field(job, "steps", 4, mapping=True).splitlines()
    starts = [i for i, line in enumerate(steps) if line.startswith("      - name:")]
    selected = [i for i in starts if _uncomment(steps[i]).split(":", 1)[1].strip() == name]
    if len(selected) != 1:
        raise GraphError("missing/duplicate plain named step: " + name)
    start = selected[0]
    end = next((i for i in starts if i > start), len(steps))
    return "\n".join(steps[start + 1:end])


def _commands(step):
    lines = step.splitlines()
    headers = [i for i, line in enumerate(lines) if line.startswith("        run:")]
    if len(headers) != 1 or _uncomment(lines[headers[0]]).strip() != "run: |":
        raise GraphError("unsupported/missing literal run block")
    commands, pending = [], ""
    for line in lines[headers[0] + 1:]:
        if not line.strip():
            continue
        if not line.startswith("          "):
            raise GraphError("unsupported run indentation/fields after run")
        raw = line[10:]
        if raw.lstrip().startswith("#"):
            if pending:
                raise GraphError("ambiguous comment inside command continuation")
            continue
        raw = _uncomment(raw)
        if not raw:
            continue
        continuation = raw.endswith("\\")
        pending += " " + (raw[:-1] if continuation else raw)
        if continuation:
            continue
        # The sole supported dispatch expression is data, not shell syntax.
        pending = pending.replace("${{ github.repository }}", "__REPOSITORY__")
        try:
            commands.append(shlex.split(pending, comments=False, posix=True))
        except ValueError as error:
            raise GraphError("unsupported shell quoting") from error
        pending = ""
    if pending:
        raise GraphError("unterminated command continuation")
    return commands


def _options(command, prefix):
    if command[:len(prefix)] != prefix:
        raise GraphError("required command is not executable in the named step")
    rest = command[len(prefix):]
    if len(rest) % 2:
        raise GraphError("unsupported option/continuation syntax")
    result = {}
    for key, value in zip(rest[::2], rest[1::2]):
        if not key.startswith("--") or key in result:
            raise GraphError("duplicate/unsupported executable option")
        result[key] = value
    return result


def _equals(actual, expected, label):
    if actual != expected:
        raise GraphError("actual " + label + " differs")


def _input(workflow, name):
    event = _field(_field(workflow, "on", 0, mapping=True), "workflow_dispatch", 2, mapping=True)
    return _field(_field(event, "inputs", 4, mapping=True), name, 6, mapping=True)


# Reviewed fade881 lexical productions; changes require deliberate source review.
# Comments/blank lines are immaterial; every executable/metadata line is closed.
ELIGIBILITY_PRODUCTION = ('    if: ${{ always() }}',
 '    name: Establish publisher eligibility',
 '    needs: [candidate, release-metadata]',
 '    runs-on: ubuntu-24.04',
 '    timeout-minutes: 10',
 '    permissions:',
 '      actions: read',
 '      contents: read',
 '    outputs:',
 '      manifest_sha256: ${{ steps.bind.outputs.manifest_sha256 }}',
 '      candidate_run_id: ${{ steps.bind.outputs.candidate_run_id }}',
 '      qualification: ${{ steps.bind.outputs.qualification }}',
 '    steps:',
 '      - name: Checkout',
 '        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
 '        with:',
 '          persist-credentials: false',
 '      - name: Setup eligibility schema Python',
 '        uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97',
 '        with:',
 "          python-version: '3.12'",
 '      - name: Install pinned eligibility schema dependencies',
 '        run: python3 -m pip install -r scripts/requirements-release.txt',
 '      - name: Download unqualified source context',
 "        if: ${{ inputs.source_context_run_id != '' && inputs.source_context_artifact_name != '' && "
 "inputs.source_context_sha256 != '' }}",
 '        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c',
 '        with:',
 '          repository: ${{ github.repository }}',
 '          run-id: ${{ inputs.source_context_run_id }}',
 '          name: ${{ inputs.source_context_artifact_name }}',
 '          github-token: ${{ github.token }}',
 '          path: source-context',
 '      - name: Validate unqualified source context before checkout',
 '        id: context',
 '        env:',
 '          CONTEXT_DIGEST: ${{ inputs.source_context_sha256 }}',
 '          CONTEXT_RUN_ID: ${{ inputs.source_context_run_id }}',
 '          CONTEXT_ARTIFACT_NAME: ${{ inputs.source_context_artifact_name }}',
 '        run: |',
 '          set -euo pipefail',
 '          python3 scripts/ci/release_private_producer.py inspect-source-context \\',
 '            --directory source-context --digest "$CONTEXT_DIGEST" \\',
 '            --repository "$GITHUB_REPOSITORY" --run-id "$CONTEXT_RUN_ID" \\',
 '            --artifact-name "$CONTEXT_ARTIFACT_NAME" --output context-outputs.txt',
 '          cat context-outputs.txt >> "$GITHUB_OUTPUT"',
 '      - name: Checkout declared frozen source read-only',
 '        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
 '        with:',
 '          ref: ${{ steps.context.outputs.frozen_sha }}',
 '          path: source-frozen',
 '          fetch-depth: 0',
 '          persist-credentials: false',
 '      - name: Checkout declared prepared source read-only',
 '        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
 '        with:',
 '          ref: ${{ steps.context.outputs.prepared_sha }}',
 '          path: source-prepared',
 '          fetch-depth: 0',
 '          persist-credentials: false',
 '      - name: Download terminal candidate authority',
 '        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c',
 '        with:',
 '          name: release-terminal-candidate-${{ inputs.transaction_id }}-${{ github.run_attempt }}',
 '          path: candidate',
 '      - name: Bind and seal terminal candidate authority',
 '        id: bind',
 '        env:',
 '          SOURCE_SHA: ${{ github.sha }}',
 '          TAG: ${{ needs.release-metadata.outputs.tag }}',
 '          CANDIDATE_RUN_ID: ${{ github.run_id }}',
 '          CANDIDATE_RESULT: ${{ needs.candidate.result }}',
 '          METADATA_RESULT: ${{ needs.release-metadata.result }}',
 '          TRANSACTION_ID: ${{ inputs.transaction_id }}',
 '          EXPECTED_POLICY_SHA256: ${{ inputs.expected_policy_sha256 }}',
 '          EXPECTED_TOPOLOGY_SHA256: ${{ inputs.expected_topology_sha256 }}',
 '          EXPECTED_RUN_ATTEMPT: ${{ inputs.expected_run_attempt }}',
 '          PRIVATE_PRERELEASE: ${{ needs.release-metadata.outputs.prerelease }}',
 '          FAIL_BEFORE_PUBLISH: ${{ inputs.fail_before_publish }}',
 '          RUN_ATTEMPT: ${{ github.run_attempt }}',
 '          CONTEXT_DIGEST: ${{ inputs.source_context_sha256 }}',
 '          CONTEXT_RUN_ID: ${{ inputs.source_context_run_id }}',
 '          CONTEXT_ARTIFACT_NAME: ${{ inputs.source_context_artifact_name }}',
 '          EXPECTED_SHA: ${{ inputs.expected_sha }}',
 '        run: |',
 '          set -euo pipefail',
 '          test "$CANDIDATE_RESULT" = success',
 '          test "$METADATA_RESULT" = success',
 '          python3 scripts/ci/release_private_producer.py \\',
 '            --candidate candidate --source-sha "$SOURCE_SHA" --expected-sha "$EXPECTED_SHA" \\',
 '            --planned-tag "$TAG" --transaction-id "$TRANSACTION_ID" \\',
 '            --repository "$GITHUB_REPOSITORY" --workflow-ref "$GITHUB_WORKFLOW_REF" \\',
 '            --run-id "$GITHUB_RUN_ID" --run-attempt "$RUN_ATTEMPT" \\',
 '            --expected-policy-sha256 "$EXPECTED_POLICY_SHA256" --expected-topology-sha256 '
 '"$EXPECTED_TOPOLOGY_SHA256" \\',
 '            --expected-run-attempt "$EXPECTED_RUN_ATTEMPT" \\',
 '            --context-directory source-context --context-digest "$CONTEXT_DIGEST" \\',
 '            --context-run-id "$CONTEXT_RUN_ID" --context-artifact-name "$CONTEXT_ARTIFACT_NAME" \\',
 '            --frozen-root source-frozen --prepared-root source-prepared \\',
 '            --policy policy/release-publication-admission.json --prerelease "$PRIVATE_PRERELEASE" --output '
 'private-producer.json',
 '          if [ "$FAIL_BEFORE_PUBLISH" = true ]; then',
 "            printf '::error::Deliberate rejected private predecessor; no publisher eligibility\\n'",
 '            exit 1',
 '          fi',
 '          printf \'qualification=not_proven\\n\' >> "$GITHUB_OUTPUT"',
 '          python3 scripts/release_terminal_manifest.py \\',
 '            --candidate candidate \\',
 '            --source-sha "$SOURCE_SHA" \\',
 '            --tag "$TAG" \\',
 '            --check',
 '          test -s candidate/dist/release-terminal-manifest.json',
 '          test -s candidate/attestation-subjects.sha256',
 '          manifest_sha256="$(sha256sum candidate/dist/release-terminal-manifest.json | awk \'{print '
 '$1}\')"',
 '          test "$(jq -r \'.source_sha\' candidate/dist/release-terminal-manifest.json)" = "$SOURCE_SHA"',
 '          printf \'manifest_sha256=%s\\n\' "$manifest_sha256" >> "$GITHUB_OUTPUT"',
 '          printf \'candidate_run_id=%s\\n\' "$CANDIDATE_RUN_ID" >> "$GITHUB_OUTPUT"',
 "          printf 'Private candidate consistency recorded for %s at run %s (manifest sha256:%s).\\n' \\",
 '            "$SOURCE_SHA" "$CANDIDATE_RUN_ID" "$manifest_sha256"',
 '      - name: Retain private producer observation',
 '        if: ${{ always() }}',
 '        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a',
 '        with:',
 '          name: private-producer-${{ inputs.transaction_id }}-${{ github.run_attempt }}',
 '          path: private-producer.json',
 '          if-no-files-found: warn',
 '          retention-days: 7')
DISPATCH_PRODUCTION = ('    name: Trigger Fail-Closed Release Transaction',
 '    needs: validate',
 '    runs-on: ubuntu-24.04',
 '    timeout-minutes: 5',
 '    permissions:',
 '      actions: write',
 '    steps:',
 '      - name: Dispatch release transaction',
 '        env:',
 '          GH_TOKEN: ${{ github.token }}',
 '          EXPECTED_POLICY_SHA256: ${{ inputs.expected_policy_sha256 }}',
 '          EXPECTED_TOPOLOGY_SHA256: ${{ inputs.expected_topology_sha256 }}',
 '          EXPECTED_RUN_ATTEMPT: ${{ inputs.expected_run_attempt }}',
 '          SOURCE_CONTEXT_RUN_ID: ${{ inputs.source_context_run_id }}',
 '          SOURCE_CONTEXT_ARTIFACT_NAME: ${{ inputs.source_context_artifact_name }}',
 '          SOURCE_CONTEXT_SHA256: ${{ inputs.source_context_sha256 }}',
 '          TRANSACTION_ID: ${{ inputs.transaction_id }}',
 '          INPUT_PRERELEASE: ${{ github.event.inputs.prerelease }}',
 '          INPUT_SKIP_CRATES: ${{ github.event.inputs.skip_crates }}',
 '          INPUT_SKIP_EXTENSION: ${{ github.event.inputs.skip_extension }}',
 '          INPUT_SKIP_DOCKER: ${{ github.event.inputs.skip_docker }}',
 '          EXPECTED_SHA: ${{ needs.validate.outputs.subject_sha }}',
 '          DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}',
 '        run: |',
 '          TAG="${{ needs.validate.outputs.tag }}"',
 '          REF="$DEFAULT_BRANCH"',
 '          PRERELEASE="$INPUT_PRERELEASE"',
 '          echo "Dispatching release workflow for ${TAG}"',
 '          gh api \\',
 '            -X POST \\',
 '            /repos/${{ github.repository }}/actions/workflows/release.yml/dispatches \\',
 '            -f ref="${REF}" \\',
 '            -f inputs[no_publish]=true -f inputs[transaction_id]="$TRANSACTION_ID" \\',
 '            -f inputs[expected_policy_sha256]="$EXPECTED_POLICY_SHA256" \\',
 '            -f inputs[expected_topology_sha256]="$EXPECTED_TOPOLOGY_SHA256" \\',
 '            -f inputs[expected_run_attempt]="$EXPECTED_RUN_ATTEMPT" \\',
 '            -f inputs[source_context_run_id]="$SOURCE_CONTEXT_RUN_ID" \\',
 '            -f inputs[source_context_artifact_name]="$SOURCE_CONTEXT_ARTIFACT_NAME" \\',
 '            -f inputs[source_context_sha256]="$SOURCE_CONTEXT_SHA256" \\',
 '            -F inputs[tag]="${TAG}" \\',
 '            -F inputs[expected_sha]="$EXPECTED_SHA" \\',
 '            -F inputs[prerelease]="${PRERELEASE}" \\',
 '            -F inputs[skip_crates]="$INPUT_SKIP_CRATES" \\',
 '            -F inputs[skip_extension]="$INPUT_SKIP_EXTENSION" \\',
 '            -F inputs[skip_docker]="$INPUT_SKIP_DOCKER"',
 '          echo "Candidate construction now precedes every tag and publisher mutation."',
 '      - name: Summary',
 '        env:',
 '          REPOSITORY: ${{ github.repository }}',
 '          SKIP_CRATES: ${{ github.event.inputs.skip_crates }}',
 '          SKIP_EXTENSION: ${{ github.event.inputs.skip_extension }}',
 '          SKIP_DOCKER: ${{ github.event.inputs.skip_docker }}',
 '        run: |',
 '          echo "## Release Triggered :rocket:" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "Only the release transaction has been dispatched." >> "$GITHUB_STEP_SUMMARY"',
 '          echo "" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "### Build & Release" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "- [Release](https://github.com/$REPOSITORY/actions/workflows/release.yml) - build, '
 'validate, attest, then publish" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "### Downstream eligibility after candidate success" >> "$GITHUB_STEP_SUMMARY"',
 '          if [ "$SKIP_CRATES" != "true" ]; then',
 '            echo "- [Publish to '
 'crates.io](https://github.com/$REPOSITORY/actions/workflows/publish-crates.yml)" >> "$GITHUB_STEP_SUMMARY"',
 '          else',
 '            echo "- ~~Publish to crates.io~~ (skipped)" >> "$GITHUB_STEP_SUMMARY"',
 '          fi',
 '          if [ "$SKIP_EXTENSION" != "true" ]; then',
 '            echo "- [Publish VSCode '
 'Extension](https://github.com/$REPOSITORY/actions/workflows/publish-extension.yml) - Marketplace and Open '
 'VSX jobs report separately" >> "$GITHUB_STEP_SUMMARY"',
 '          else',
 '            echo "- ~~Publish VSCode Extension~~ (skipped)" >> "$GITHUB_STEP_SUMMARY"',
 '          fi',
 '          if [ "$SKIP_DOCKER" != "true" ]; then',
 '            echo "- [Publish Docker '
 'Images](https://github.com/$REPOSITORY/actions/workflows/docker-publish.yml)" >> "$GITHUB_STEP_SUMMARY"',
 '          else',
 '            echo "- ~~Publish Docker Images~~ (skipped)" >> "$GITHUB_STEP_SUMMARY"',
 '          fi',
 '          echo "" >> "$GITHUB_STEP_SUMMARY"',
 '          echo "No downstream publisher is dispatched until release.yml\'s terminal candidate succeeds." '
 '>> "$GITHUB_STEP_SUMMARY"')
RELEASE_ENV_PRODUCTION = ('  CARGO_TERM_COLOR: always', '  RUST_BACKTRACE: 1')
ORCHESTRATION_ENV_PRODUCTION = ('  CARGO_TERM_COLOR: always', '  RUST_BACKTRACE: 1')

def _closed_production(block, expected, label):
    actual = tuple(_uncomment(line) for line in block.splitlines() if _uncomment(line).strip())
    _equals(actual, expected, label + " supported lexical production")

def _default_shell_boundary(workflow):
    if "defaults" in _plain_fields(workflow, 0):
        raise GraphError("unsupported workflow defaults")



def _job_shell_boundary(job):
    # Defaults are unsupported for every job, including future publishers.
    # Shells affect command execution, not GitHub's job-condition evaluation.
    if "defaults" in _plain_fields(job, 4):
        raise GraphError("unsupported job defaults")
    steps = _field(job, "steps", 4, mapping=True).splitlines()
    starts = [i for i, line in enumerate(steps) if line.startswith("      - ")]
    for number, start in enumerate(starts):
        if not re.fullmatch(r"      - name: [^\s].*", _uncomment(steps[start])):
            raise GraphError("unsupported step list header; plain named steps required")
        end = starts[number + 1] if number + 1 < len(starts) else len(steps)
        fields = _plain_fields("\n".join(steps[start + 1:end]), 8)
        if "shell" in fields and fields["shell"] != "bash":
            raise GraphError("unsupported custom step shell")



def _read_only_permissions(block, key_indent):
    permissions = _field(block, "permissions", key_indent, mapping=True)
    fields = _plain_fields(permissions, key_indent + 2)
    if not fields or any(key not in ("actions", "contents") or value != "read"
                         for key, value in fields.items()):
        raise GraphError("unsupported private permission authority")
    # Nested values or different indentation cannot hide additional authority.
    if any(_uncomment(line).strip() and len(line) - len(line.lstrip()) != key_indent + 2
           for line in permissions.splitlines()):
        raise GraphError("unsupported nested permission style")



def _upstream_production(block):
    """Ignore only YAML trivia outside opaque literal/folded scalar bodies."""
    result, scalar_indent = [], None
    for line in block.splitlines():
        indent = len(line) - len(line.lstrip())
        if scalar_indent is not None:
            if not line.strip() or indent > scalar_indent:
                result.append(line)
                continue
            scalar_indent = None
        live = _uncomment(line)
        if not live.strip():
            continue
        result.append(live)
        if re.search(r":\s*[|>][-+0-9]*\s*$", live):
            scalar_indent = indent
    return "\n".join(result).encode("utf-8")


# Fixed reviewed b5c6 productions; never computed from validator input.
SUPPORTED_JOBS = ('release-metadata', 'build', 'candidate', 'publisher-eligibility', 'publish-release', 'dispatch-publishers')
UPSTREAM_PRODUCTION_SHA256 = {'release-metadata': '526a1b8b7382517b152724c2b3aa893df0a67d883c02ce096f5941bf5e98c442', 'build': '83b7fa4bd16cde8f0f808d701aed701fe021f81b47eaed8185c0f32271f2473c', 'candidate': '62254ea65fd1558c143bb4df534a9846f775ab6e8ec09d1e366f86ad43358731'}
# Reviewed 32a59036 validate job, including opaque scalar bodies; never input-derived.
SUPPORTED_ORCHESTRATION_JOBS = ('validate', 'trigger-release')
ORCHESTRATION_VALIDATE_SHA256 = '2a484848f8916aba5214059cb0336b793c08cf37be92a106459f9199a6095523'

def validate_graph(workflow):
    _default_shell_boundary(workflow)
    _read_only_permissions(workflow, 0)
    _closed_production(_field(workflow, "env", 0, mapping=True), RELEASE_ENV_PRODUCTION, "global environment")
    actual_jobs = tuple(re.findall(r"(?m)^  ([A-Za-z0-9_-]+):$", _jobs_text(workflow)))
    _equals(set(actual_jobs), set(SUPPORTED_JOBS), "declared job membership")
    for name, digest in UPSTREAM_PRODUCTION_SHA256.items():
        actual = hashlib.sha256(_upstream_production(_job_block(workflow, name))).hexdigest()
        _equals(actual, digest, name + " reviewed upstream production")
    for match in re.finditer(r"(?m)^  ([A-Za-z0-9_-]+):$", _jobs_text(workflow)):
        _job_shell_boundary(_job_block(workflow, match.group(1)))
    private_input = _input(workflow, "no_publish")
    _equals(_field(private_input, "type", 8), "boolean", "private input type")
    _equals(_field(private_input, "default", 8), "true", "private input default")
    candidate = _job_block(workflow, "candidate")
    eligibility = _job_block(workflow, "publisher-eligibility")
    _closed_production(eligibility, ELIGIBILITY_PRODUCTION, "eligibility job")
    _needs(candidate, ("build", "release-metadata"))
    _needs(eligibility, ("candidate", "release-metadata"))
    _equals(_field(eligibility, "if", 4), "${{ always() }}", "terminal fan-in condition")
    outputs = _field(eligibility, "outputs", 4, mapping=True)
    _equals(_field(outputs, "qualification", 6), "${{ steps.bind.outputs.qualification }}", "qualification output")
    # Retain the conservative pre-eligibility mutation/permission fence.
    for name in ("release-metadata", "build", "candidate", "publisher-eligibility"):
        job = _job_block(workflow, name)
        fields = _plain_fields(job, 4)
        if "environment" in fields:
            raise GraphError("unsupported private job environment")
        if "permissions" in fields:
            _read_only_permissions(job, 4)
        live = "\n".join(_uncomment(line) for line in job.splitlines() if not line.lstrip().startswith("#"))
        if any(token in live for token in ("id-token: write", "attestations: write", "contents: write", "artifact-metadata: write", "environment:", "actions/attest@", "gh api -X POST", "git push")):
            raise GraphError("pre-eligibility public authority")
    bind = _step(eligibility, "Bind and seal terminal candidate authority")
    env = _field(bind, "env", 8, mapping=True)
    bindings = {"CANDIDATE_RESULT": "needs.candidate.result", "METADATA_RESULT": "needs.release-metadata.result",
                "EXPECTED_SHA": "inputs.expected_sha", "TRANSACTION_ID": "inputs.transaction_id",
                "RUN_ATTEMPT": "github.run_attempt", "FAIL_BEFORE_PUBLISH": "inputs.fail_before_publish"}
    for variable, expression in bindings.items():
        _equals(_field(env, variable, 10), "${{ " + expression + " }}", variable + " binding")
    commands = _commands(bind)
    if len(commands) != 17:
        raise GraphError("unsupported private gate command layout")
    _equals(commands[:3], [["set", "-euo", "pipefail"], ["test", "$CANDIDATE_RESULT", "=", "success"],
                          ["test", "$METADATA_RESULT", "=", "success"]], "standalone predecessor guards")
    options = _options(commands[3], ["python3", "scripts/ci/release_private_producer.py"])
    for key, value in {"--expected-sha": "$EXPECTED_SHA", "--transaction-id": "$TRANSACTION_ID",
                       "--run-attempt": "$RUN_ATTEMPT", "--policy": "policy/release-publication-admission.json",
                       "--frozen-root": "source-frozen", "--prepared-root": "source-prepared"}.items():
        _equals(options.get(key), value, "producer " + key)
    _equals(commands[4], ["if", "[", "$FAIL_BEFORE_PUBLISH", "=", "true", "];", "then"], "failure injection guard")
    _equals(commands[6:9], [["exit", "1"], ["fi"], ["printf", "qualification=not_proven\\n", ">>", "$GITHUB_OUTPUT"]], "failure/qualification commands")
    for command in commands:
        if "$GITHUB_OUTPUT" in command and any("qualification=" in token for token in command):
            _equals(command, ["printf", "qualification=not_proven\\n", ">>", "$GITHUB_OUTPUT"], "qualification write")
    download = _field(_step(eligibility, "Download unqualified source context"), "with", 8, mapping=True)
    for key, value in {"run-id": "${{ inputs.source_context_run_id }}", "name": "${{ inputs.source_context_artifact_name }}"}.items():
        _equals(_field(download, key, 10), value, "context " + key)
    inspect_step = _step(eligibility, "Validate unqualified source context before checkout")
    inspect_env = _field(inspect_step, "env", 8, mapping=True)
    for variable, field in (("CONTEXT_DIGEST", "source_context_sha256"), ("CONTEXT_RUN_ID", "source_context_run_id"), ("CONTEXT_ARTIFACT_NAME", "source_context_artifact_name")):
        _equals(_field(inspect_env, variable, 10), "${{ inputs." + field + " }}", "inspection environment")
    inspect = _commands(inspect_step)
    if len(inspect) != 3:
        raise GraphError("unsupported context inspection command layout")
    _equals(inspect[0], ["set", "-euo", "pipefail"], "inspection strict shell")
    _equals(inspect[2], ["cat", "context-outputs.txt", ">>", "$GITHUB_OUTPUT"], "inspection outputs")
    inspect_options = _options(inspect[1], ["python3", "scripts/ci/release_private_producer.py", "inspect-source-context"])
    _equals(inspect_options.get("--digest"), "$CONTEXT_DIGEST", "context digest argument")
    for role in ("frozen", "prepared"):
        checkout = _field(_step(eligibility, "Checkout declared " + role + " source read-only"), "with", 8, mapping=True)
        _equals(_field(checkout, "ref", 10), "${{ steps.context.outputs." + role + "_sha }}", role + " source ref")
        _equals(_field(checkout, "persist-credentials", 10), "false", role + " credential persistence")
    condition = "${{ inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
    for name, dependencies in (("publish-release", ("candidate", "publisher-eligibility", "release-metadata")),
                               ("dispatch-publishers", ("publish-release", "publisher-eligibility", "release-metadata"))):
        publisher = _job_block(workflow, name)
        _equals(_field(publisher, "if", 4), condition, "publisher qualification condition")
        _needs(publisher, dependencies)
    for match in re.finditer(r"(?m)^  ([A-Za-z0-9_-]+):$", _jobs_text(workflow)):
        name = match.group(1)
        if name not in PUBLISHERS:
            live = "\n".join(_uncomment(line) for line in _job_block(workflow, name).splitlines())
            if any(token in live for token in ("actions/attest@", "action-gh-release@", "gh api -X POST", "git push")):
                raise GraphError("unregistered public mutation job")


def validate_dispatch(workflow):
    _default_shell_boundary(workflow)
    _read_only_permissions(workflow, 0)
    jobs = tuple(re.findall(r"(?m)^  ([A-Za-z0-9_-]+):$", _jobs_text(workflow)))
    _equals(set(jobs), set(SUPPORTED_ORCHESTRATION_JOBS), "orchestration job membership")
    validate = _job_block(workflow, "validate")
    digest = hashlib.sha256(_upstream_production(validate)).hexdigest()
    _equals(digest, ORCHESTRATION_VALIDATE_SHA256, "reviewed orchestration validation production")
    for name in jobs:
        _job_shell_boundary(_job_block(workflow, name))
    _closed_production(_field(workflow, "env", 0, mapping=True), ORCHESTRATION_ENV_PRODUCTION, "orchestration global environment")
    trigger = _job_block(workflow, "trigger-release")
    _closed_production(trigger, DISPATCH_PRODUCTION, "dispatch job")
    dispatch = _step(trigger, "Dispatch release transaction")
    env = _field(dispatch, "env", 8, mapping=True)
    fields = ("source_context_run_id", "source_context_artifact_name", "source_context_sha256")
    for field in fields:
        _equals(_field(_input(workflow, field), "type", 8), "string", "context input type")
        _equals(_field(env, field.upper(), 10), "${{ inputs." + field + " }}", "context environment")
    commands = _commands(dispatch)
    # The supported dispatch is top-level. Conditional wrappers, heredocs,
    # command groups and extra marker-printing commands are not admitted.
    if len(commands) != 6 or commands[4][:2] != ["gh", "api"]:
        raise GraphError("unsupported/ambiguous executable child dispatch layout")
    _equals(commands[:3], [["TAG=${{ needs.validate.outputs.tag }}"], ["REF=$DEFAULT_BRANCH"],
                           ["PRERELEASE=$INPUT_PRERELEASE"]], "dispatch source assignments")
    if commands[3][:1] != ["echo"] or commands[5][:1] != ["echo"]:
        raise GraphError("unsupported dispatch command layout")
    command = commands[4][2:]
    if command[:3] != ["-X", "POST", "/repos/__REPOSITORY__/actions/workflows/release.yml/dispatches"]:
        raise GraphError("unsupported child dispatch route")
    arguments = command[3:]
    if len(arguments) % 2:
        raise GraphError("unsupported dispatch continuation")
    values = {}
    for flag, assignment in zip(arguments[::2], arguments[1::2]):
        if flag not in ("-f", "-F") or "=" not in assignment:
            raise GraphError("unsupported dispatch argument")
        key, value = assignment.split("=", 1)
        if key in values:
            raise GraphError("duplicate dispatch argument")
        values[key] = value
    for field in fields:
        _equals(values.get("inputs[" + field + "]"), "$" + field.upper(), "context forwarding")
    _equals(values.get("inputs[no_publish]"), "true", "private dispatch mode")


def main():
    try:
        validate_graph(WORKFLOW.read_text(encoding="utf-8"))
        validate_dispatch(ORCHESTRATION.read_text(encoding="utf-8"))
    except (GraphError, OSError) as error:
        print("release prerequisite graph check failed: " + str(error), file=sys.stderr)
        return 1
    print("private source grammar valid; qualification and hosted mutation counts NOT_PROVEN")
    return 0


if __name__ == "__main__":
    sys.exit(main())
