"""Paired falsifiers mutate the actual selected workflow, not a toy graph."""
from pathlib import Path
import unittest

import check_release_prerequisite_graph as graph


class PrivateGraphTests(unittest.TestCase):
    def test_actual_graph_and_independent_mutation_refusals(self):
        source = graph.WORKFLOW.read_text(encoding='utf-8')
        graph.validate_graph(source)
        condition = "inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied'"
        controls = [
            ('default: true', 'default: false'),
            ('\n    if: ${{ always() }}', '\n    if: ${{ success() }}'),
            ('test "$CANDIDATE_RESULT" = success', 'true'),
            ('test "$METADATA_RESULT" = success', 'true'),
            ('--expected-sha "$EXPECTED_SHA"', '--expected-sha "$SOURCE_SHA"'),
            ('--transaction-id "$TRANSACTION_ID"', '--transaction-id other'),
            ('--run-attempt "$RUN_ATTEMPT"', '--run-attempt 1'),
            ('qualification=not_proven', 'qualification=satisfied'),
            ('run-id: ${{ inputs.source_context_run_id }}', 'run-id: 1'),
            ('name: ${{ inputs.source_context_artifact_name }}', 'name: latest'),
            ('--digest "$CONTEXT_DIGEST"', '--digest other'),
            ('ref: ${{ steps.context.outputs.frozen_sha }}', 'ref: main'),
            ('--frozen-root source-frozen --prepared-root source-prepared', '--frozen-root . --prepared-root .'),
            (condition, 'true'),
            # no_publish=false cannot replace a NOT_PROVEN qualification result.
            (condition, 'inputs.no_publish == false'),
            ('    needs: [candidate, publisher-eligibility, release-metadata]',
             '    needs: [candidate, release-metadata]'),
            ('  candidate:\n', '  candidate:\n    environment: publisher\n'),
            ('  candidate:\n', '  candidate:\n    permissions:\n      id-token: write\n'),
            ('if [ "$FAIL_BEFORE_PUBLISH" = true ]; then', 'if false; then'),
        ]
        for original, replacement in controls:
            if original not in source:
                raise RuntimeError('mutation selector disappeared: ' + original)
            changed = source.replace(original, replacement, 1)
            try:
                graph.validate_graph(changed)
            except graph.GraphError:
                continue
            raise RuntimeError('unsafe actual graph accepted: ' + original)


    def test_actual_dispatch_preserves_each_declared_context_field(self):
        source = graph.ORCHESTRATION.read_text(encoding="utf-8")
        graph.validate_dispatch(source)
        for field in ("source_context_run_id", "source_context_artifact_name", "source_context_sha256"):
            token = '-f inputs[' + field + ']="$' + field.upper() + '"'
            if token not in source:
                raise RuntimeError("dispatch selector disappeared: " + field)
            try:
                graph.validate_dispatch(source.replace(token, "", 1))
            except graph.GraphError:
                continue
            raise RuntimeError("missing context forwarding accepted: " + field)


if __name__ == '__main__':
    unittest.main()
