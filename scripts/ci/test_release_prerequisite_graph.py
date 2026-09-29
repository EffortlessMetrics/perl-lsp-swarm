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


    def test_current_source_comment_and_quoted_decoys_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        condition = "    if: ${{ inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
        controls = [
            (condition, "    if: true\n    #" + condition),
            ("    needs: [candidate, publisher-eligibility, release-metadata]", "    needs: [candidate, release-metadata] # publisher-eligibility"),
            ("    needs: [build, release-metadata]", "    needs: [release-metadata]\n    #    needs: [build, release-metadata]"),
            ("    needs: [candidate, release-metadata]", "    needs: [release-metadata]\n    #    needs: [candidate, release-metadata]"),
            ('          test "$CANDIDATE_RESULT" = success', '          echo \'test "$CANDIDATE_RESULT" = success\''),
            ('          test "$METADATA_RESULT" = success', '          # test "$METADATA_RESULT" = success\n          true'),
            ('--expected-sha "$EXPECTED_SHA"', '--expected-sha "$SOURCE_SHA"'),
            ('        default: true', '        default: false #        default: true'),
        ]
        for original, replacement in controls:
            if original not in source:
                raise RuntimeError("decoy selector disappeared: " + original)
            changed = source.replace(original, replacement, 1)
            if original == '--expected-sha "$EXPECTED_SHA"':
                changed = changed.replace('          set -euo pipefail', '          # --expected-sha "$EXPECTED_SHA"\n          set -euo pipefail', 1)
                start = changed.index('  publisher-eligibility:')
                changed = changed[:start] + changed[start:].replace('          set -euo pipefail', '          echo \'--expected-sha "$EXPECTED_SHA"\'\n          set -euo pipefail', 1)
            with self.subTest(selector=original):
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(changed)

    def test_dispatch_arguments_cannot_survive_only_as_comment_or_echo(self):
        source = graph.ORCHESTRATION.read_text(encoding="utf-8")
        for field in ("source_context_run_id", "source_context_artifact_name", "source_context_sha256"):
            token = '-f inputs[' + field + ']="$' + field.upper() + '"'
            for decoy in ("          # " + token, "          echo '" + token + "'"):
                changed = source.replace(token, '-f inputs[unrelated]=other', 1)
                changed = changed.replace('          gh api', decoy + "\n          gh api", 1)
                with self.subTest(field=field, decoy=decoy):
                    with self.assertRaises(graph.GraphError):
                        graph.validate_dispatch(changed)


    def test_restricted_styles_and_quoted_yaml_marker_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        condition = "    if: ${{ inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
        controls = [
            (condition, "    if: true\n    env:\n      PROOF_HINT: "+repr(condition)),
            ("    needs: [build, release-metadata]", "    needs: [release-metadata]\n    env:\n      PROOF_HINT: '    needs: [build, release-metadata]'"),
            ("        default: true", "        default: false"),
            (condition, condition + "\n    if: true"),
            ("    needs: [build, release-metadata]", "    needs: [build, release-metadata]\n    needs: [release-metadata]"),
            ("    needs: [build, release-metadata]", "    needs: *dependencies"),
            ("          test -s candidate/dist/release-terminal-manifest.json", "          printf 'qualification=satisfied\\n' >> \"$GITHUB_OUTPUT\""),
        ]
        for original, replacement in controls:
            with self.subTest(selector=original, replacement=replacement):
                with self.assertRaises(graph.GraphError):
                    changed = source.replace(original, replacement, 1)
                    if original == "        default: true":
                        changed = changed.replace("description: 'Private rehearsal only; no public authority'", "description: '        default: true'", 1)
                    graph.validate_graph(changed)
        orchestration = graph.ORCHESTRATION.read_text(encoding="utf-8")
        wrapped = orchestration.replace("          gh api", "          if false; then\n          gh api", 1)
        wrapped = wrapped.replace('          echo "Candidate construction', '          fi\n          echo "Candidate construction', 1)
        with self.assertRaises(graph.GraphError):
            graph.validate_dispatch(wrapped)


    def test_unvalidated_suffix_and_step_authority_mutants_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        condition = "    if: ${{ inputs.no_publish == false && needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
        controls = [
            ('          test -s candidate/dist/release-terminal-manifest.json', '          printf \'%s=%s\\n\' qualification satisfied >> "$GITHUB_OUTPUT"'),
            ('          test -s candidate/dist/release-terminal-manifest.json', '          printf \'%s\\n\' "$(printf qualification; printf =; printf satisfied)" >> "$GITHUB_OUTPUT"'),
            (condition, condition + '\n    "if": true'),
            ('    needs: [build, release-metadata]', '    needs: [build, release-metadata]\n    "needs": [release-metadata]'),
            ('        id: bind', '        id: other'),
            ('        id: bind', '        id: bind\n        shell: bash -c true {0}'),
        ]
        for original, replacement in controls:
            with self.subTest(replacement=replacement):
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source.replace(original, replacement, 1))



    def test_closed_prior_steps_defaults_and_environment_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        controls = [
            ("jobs:", "defaults:\n  run:\n    shell: bash -c true {0}\njobs:"),
            ("  CARGO_TERM_COLOR: always", "  CARGO_TERM_COLOR: always\n  BASH_ENV: injected.sh"),
            ("    name: Establish publisher eligibility", "    name: Establish publisher eligibility\n    defaults:\n      run:\n        shell: bash -c true {0}"),
            ("        id: context", "        id: context\n        env: {BASH_ENV: injected.sh}"),
            ("        id: bind", "        id: bind\n        \"id\": other"),
            ("      - name: Bind and seal terminal candidate authority", "      - name: Inject qualification\n        id: bind\n        run: |\n          printf 'qualification=satisfied' >> \"$GITHUB_OUTPUT\"\n      - name: Bind and seal terminal candidate authority"),
            ("      - name: Retain private producer observation", "      - name: Retain private producer observation\n        shell: bash -c true {0}"),
        ]
        for original, replacement in controls:
            with self.subTest(replacement=replacement):
                self.assertIn(original, source)
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source.replace(original, replacement, 1))
        # Comments may change; executable and authority metadata may not.
        graph.validate_graph(source.replace("        id: bind", "        # reviewed authority binding\n        id: bind"))
        orchestration = graph.ORCHESTRATION.read_text(encoding="utf-8")
        with self.assertRaises(graph.GraphError):
            graph.validate_dispatch(orchestration.replace("      - name: Dispatch release transaction", "      - name: Dispatch release transaction\n        shell: bash -c true {0}", 1))



    def test_upstream_job_defaults_and_custom_step_shells_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        for name in ("candidate", "build", "release-metadata"):
            header = "  " + name + ":"
            with self.subTest(job=name, seam="defaults"):
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source.replace(header, header + "\n    defaults:\n      run:\n        shell: bash -c true {0}", 1))
        with self.assertRaises(graph.GraphError):
            graph.validate_graph(source.replace("        shell: bash", "        shell: bash -c true {0}", 1))
        with self.assertRaises(graph.GraphError):
            graph.validate_graph(source.replace("        shell: bash", '        shell: bash\n        "shell": bash -c true {0}', 1))
        graph.validate_graph(source)



    def test_quoted_permission_authority_decoys_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        start = source.index("  candidate:")
        end = source.index("  publisher-eligibility:", start)
        block = source[start:end]
        for authority in ('"contents": write', 'contents: "write"', '"id-token": write', '"attestations": write'):
            with self.subTest(authority=authority):
                changed = block.replace("      contents: read", "      " + authority, 1)
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source[:start] + changed + source[end:])
        with self.assertRaises(graph.GraphError):
            graph.validate_graph(source.replace("  contents: read", '  contents: "write"', 1))
        with self.assertRaises(graph.GraphError):
            graph.validate_graph(source.replace("  candidate:", '  candidate:\n    "environment": release', 1))



    def test_fixed_upstream_productions_and_job_membership_refuse(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        for command in ('gh release create v0.0.0', 'gh api --method POST /repos/example/releases'):
            with self.subTest(command=command):
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source + "\n  extra-public-route:\n    runs-on: ubuntu-24.04\n    steps:\n      - name: Public mutation\n        run: |\n          " + command + "\n")
        for name in ("release-metadata", "build", "candidate"):
            header = "  " + name + ":"
            for injection in ('    env:\n      TOKEN: ${{ secrets.PUBLIC_TOKEN }}', '    secrets: inherit'):
                with self.subTest(job=name, injection=injection):
                    with self.assertRaises(graph.GraphError):
                        graph.validate_graph(source.replace(header, header + "\n" + injection, 1))
        start = source.index("  candidate:")
        end = source.index("  publisher-eligibility:", start)
        block = source[start:end]
        for needle, changed in [
            ("uses: actions/checkout@", "uses: arbitrary/public-action@"),
            ("        run: |", "        run: |\n          gh release create v0.0.0"),
        ]:
            self.assertIn(needle, block)
            with self.assertRaises(graph.GraphError):
                graph.validate_graph(source[:start] + block.replace(needle, changed, 1) + source[end:])
        # YAML-only comments are safe trivia; opaque scalar comments/blanks are data.
        graph.validate_graph(source.replace("  build:", "  build:\n    # reviewed upstream job", 1))
        graph.validate_graph(source.replace("    needs: [build, release-metadata]", "    # dependency annotation\n    needs: [build, release-metadata]", 1))
        for scalar in ("run: |", "run: >-"):
            with self.subTest(scalar=scalar):
                with self.assertRaises(graph.GraphError):
                    graph.validate_graph(source.replace("        " + scalar, "        " + scalar + "\n          # opaque scalar data", 1))
        graph.validate_graph(source)



    def test_custom_shell_as_public_script_step_header_refuses(self):
        source = graph.WORKFLOW.read_text(encoding="utf-8")
        header = "      - name: Atomically create exact release tag"
        self.assertIn(header, source)
        with self.assertRaises(graph.GraphError):
            graph.validate_graph(source.replace(header, "      - shell: bash -c true {0}", 1))
        graph.validate_graph(source)


if __name__ == '__main__':
    unittest.main()
