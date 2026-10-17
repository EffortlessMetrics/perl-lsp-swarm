"""Compiler-free contracts for the existing storage self-test job (#17667).

The literal conditions below use GitHub's documented status-function semantics.
The recording model tests this job's dependency edges, not the Actions service.
No workflow commands, Cargo builds, or lifecycle experiments are launched here.
"""
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/ci-gate-self-tests.yml'
JOB = 'cargo-admitted-storage-self-test'
CHECKOUT_ID = 'storage_checkout'
INDEPENDENT = "${{ !cancelled() && steps.storage_checkout.outcome == 'success' }}"
SELF_TEST = 'test_storage_self_test_workflow.py'
PORTABLE = (
    SELF_TEST,
    'test_cargo_admitted*.py',
    'test_cargo_cohort_source_plan.py',
    'test_cargo_cohort.py',
    'test_routed_test_cohorts.py',
    'test_storage_doctor.py',
    'test_measure_cargo_storage.py',
    'test_routed_storage_qualification_report.py',
)
TOOLCHAIN = 'Install pinned Rust toolchain for Cargo runtime fixture'
NATIVE = {
    'Exercise Cargo binary runtime environments through admitted owner': (
        'set -euo pipefail\n'
        'cd scripts/tests/fixtures/routed-resolver-2/b\n'
        '../../../../cargo-admitted test --locked --offline -p cohort-b --test shared\n'
        '../../../../cargo-admitted test --locked --offline -p cohort-b --features enabled-bin --test runtime_env -- --nocapture\n'
        '../../../../cargo-admitted test --locked --offline -p cohort-b --features enabled-bin --tests\n'
        '../../../../cargo-admitted bench --locked --offline -p cohort-b --features enabled-bin --bench runtime_env'
    ),
    'Run the native producer unit tests through admitted Cargo':
        'scripts/cargo-admitted test --locked -p xtask --bin routed-test-cohorts',
    'Check native producer lints through admitted Cargo':
        'scripts/cargo-admitted clippy -p xtask --all-targets --profile agent --locked -- -D warnings',
}


def storage_steps(text):
    """Read this job's plain step fields; deliberately not a general YAML parser."""
    matches = list(re.finditer(r'^  ' + re.escape(JOB) + r':\n(.*?)(?=^  \S|\Z)', text, re.M | re.S))
    if len(matches) != 1:
        raise AssertionError('expected exactly one storage self-test job')
    job = matches[0].group(1)
    if re.search(r'^    (?:if|needs|continue-on-error):', job, re.M):
        raise AssertionError('storage job must not hide an independent result')
    blocks = re.split(r'^      - name: ', job, flags=re.M)[1:]
    steps = []
    for block in blocks:
        lines = block.splitlines()
        step = {'name': lines[0], 'source': block}
        for index, line in enumerate(lines[1:], 1):
            match = re.fullmatch(r'        ([a-z-]+): (.*)', line)
            if match is None:
                continue
            key, value = match.groups()
            if key in step:
                raise AssertionError('duplicate field: ' + key)
            if value == '|' and key == 'run':
                command = []
                for following in lines[index + 1:]:
                    if following and not following.startswith('          '):
                        break
                    command.append(following[10:])
                value = '\n'.join(command).rstrip('\n')
            step[key] = value
        steps.append(step)
    if not steps or len({step['name'] for step in steps}) != len(steps):
        raise AssertionError('missing or duplicate storage steps')
    return steps


def mapping_block(text, key, indent):
    """Select one literal mapping in this workflow's supported block shape."""
    name = re.escape(key)
    pattern = r"(?m)^ {%d}(?:%s|'%s'|\"%s\"):[ \t]*(?:#[^\n]*)?(?:\n|\Z)" % (indent, name, name, name)
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        raise AssertionError('missing or duplicate workflow mapping: ' + key)
    lines = []
    for line in text[matches[0].end():].splitlines():
        if line.strip() and not line.lstrip().startswith('#'):
            if len(line) - len(line.lstrip(' ')) <= indent:
                break
        lines.append(line)
    return '\n'.join(lines)


def literal_scalar(value):
    """Read only plain or simply quoted scalars; unsupported YAML fails closed."""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        quote = value[0]
        value = value[1:-1]
        if quote in value or '\\' in value:
            raise AssertionError('unsupported quoted workflow scalar')
        return value
    if not value or any(char in value for char in "#[]{}&*!|>'\"\\"):
        raise AssertionError('unsupported workflow scalar')
    return value


def pull_request_paths(text):
    events = mapping_block(text, 'on', 0)
    pull_request = mapping_block(events, 'pull_request', 2)
    paths = mapping_block(pull_request, 'paths', 4)
    result = []
    for line in paths.splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        match = re.fullmatch(r'      -[ \t]+(.+)', line)
        if match is None:
            raise AssertionError('unsupported pull_request.paths entry')
        result.append(literal_scalar(match[1]))
    return result


def toolchain_inputs(step):
    inputs = mapping_block(step['source'], 'with', 8)
    result = {}
    for line in inputs.splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        match = re.fullmatch(r'          ([a-z-]+):[ \t]*(.+)', line)
        if match is None or match[1] in result:
            raise AssertionError('unsupported or duplicate Rust toolchain input')
        result[match[1]] = literal_scalar(match[2])
    return result


def command(pattern):
    return "python3 -m unittest discover -s scripts/tests -p '" + pattern + "' -v"


def record_steps(steps, failure=None, cancel_before=None):
    """Recording-only model of this job's two admitted condition shapes."""
    outcomes = {}
    failed = False
    cancelled = False
    visited = []
    for step in steps:
        name = step['name']
        cancelled = cancelled or name == cancel_before
        condition = step.get('if')
        if condition == INDEPENDENT:
            allowed = not cancelled and outcomes.get(CHECKOUT_ID) == 'success'
        elif condition is None:
            allowed = not failed and not cancelled
        else:
            raise AssertionError('unexpected execution condition: ' + condition)
        outcome = 'skipped'
        if allowed:
            visited.append(name)
            outcome = 'failure' if name == failure else 'success'
            failed = failed or outcome == 'failure'
        if 'id' in step:
            outcomes[step['id']] = outcome
    return visited, 'cancelled' if cancelled else 'failure' if failed else 'success'


class StorageSelfTestWorkflow(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = WORKFLOW.read_text(encoding='utf-8')
        cls.steps = storage_steps(cls.text)

    def selected(self, pattern):
        matches = [step for step in self.steps if step.get('run') == command(pattern)]
        self.assertEqual(len(matches), 1, 'missing, duplicated or changed portable command: ' + pattern)
        return matches[0]

    def test_checkout_is_the_only_portable_prerequisite(self):
        self.assertEqual(self.steps[0]['name'], 'Checkout')
        self.assertEqual(self.steps[0].get('id'), CHECKOUT_ID)
        self.assertEqual(sum(step.get('id') == CHECKOUT_ID for step in self.steps), 1)
        for pattern in PORTABLE:
            with self.subTest(pattern=pattern):
                self.assertEqual(self.selected(pattern).get('if'), INDEPENDENT)

    def test_all_portable_controls_run_before_toolchain_installation(self):
        index = next(i for i, step in enumerate(self.steps) if step['name'] == TOOLCHAIN)
        for pattern in PORTABLE:
            with self.subTest(pattern=pattern):
                self.assertLess(self.steps.index(self.selected(pattern)), index)

    def test_no_failure_suppression_is_added(self):
        for step in self.steps:
            with self.subTest(step=step['name']):
                self.assertNotIn('continue-on-error', step)
                self.assertNotIn('|| true', step.get('run', ''))
                self.assertNotIn('set +e', step.get('run', ''))
                self.assertNotIn('always()', step.get('if', ''))

    def test_existing_rust_commands_and_dependency_order_are_preserved(self):
        names = [step['name'] for step in self.steps]
        toolchain = self.steps[names.index(TOOLCHAIN)]
        self.assertNotIn('if', toolchain)
        self.assertEqual(toolchain_inputs(toolchain), {'toolchain': '1.95.0', 'components': 'clippy'})
        previous = names.index(TOOLCHAIN)
        for name, expected in NATIVE.items():
            with self.subTest(step=name):
                self.assertEqual(names.count(name), 1)
                index = names.index(name)
                self.assertGreater(index, previous)
                self.assertEqual(self.steps[index].get('run'), expected)
                self.assertNotIn('if', self.steps[index])
                previous = index

    def test_healthy_run_reaches_every_existing_obligation(self):
        visited, result = record_steps(self.steps)
        self.assertEqual(visited, [step['name'] for step in self.steps])
        self.assertEqual(result, 'success')

    def test_failed_portable_suite_does_not_hide_independent_suites(self):
        for failed_pattern in PORTABLE:
            with self.subTest(failed=failed_pattern):
                visited, result = record_steps(self.steps, failure=self.selected(failed_pattern)['name'])
                self.assertEqual(result, 'failure')
                for pattern in PORTABLE:
                    self.assertIn(self.selected(pattern)['name'], visited)
                self.assertNotIn(TOOLCHAIN, visited)
                self.assertTrue(set(NATIVE).isdisjoint(visited))

    def test_rust_failures_cannot_prevent_completed_portable_controls(self):
        for failure in (TOOLCHAIN, *NATIVE):
            with self.subTest(failed=failure):
                visited, result = record_steps(self.steps, failure=failure)
                self.assertEqual(result, 'failure')
                self.assertIn(failure, visited)
                for pattern in PORTABLE:
                    self.assertIn(self.selected(pattern)['name'], visited)
                index = next(i for i, step in enumerate(self.steps) if step['name'] == failure)
                after_failure = [step['name'] for step in self.steps[index + 1:]]
                self.assertTrue(set(after_failure).isdisjoint(visited))

    def test_failed_checkout_does_not_start_test_work(self):
        visited, result = record_steps(self.steps, failure='Checkout')
        self.assertEqual(visited, ['Checkout'])
        self.assertEqual(result, 'failure')

    def test_cancellation_stops_new_work_at_every_boundary(self):
        for index, step in enumerate(self.steps):
            with self.subTest(cancel_before=step['name']):
                visited, result = record_steps(self.steps, cancel_before=step['name'])
                self.assertEqual(visited, [row['name'] for row in self.steps[:index]])
                self.assertEqual(result, 'cancelled')

    def test_own_contract_and_workflow_changes_are_selected(self):
        paths = pull_request_paths(self.text)
        self.assertEqual(paths.count('scripts/tests/' + SELF_TEST), 1)
        self.assertEqual(paths.count('.github/workflows/ci-gate-self-tests.yml'), 1)
        self.selected(SELF_TEST)

    def test_path_mentions_outside_pull_request_paths_do_not_count(self):
        path = 'scripts/tests/' + SELF_TEST
        original = "      - '" + path + "'\n"
        fixture = 'on:\n  pull_request:\n    paths:\n' + original
        for replacement in ('', "      # " + path + "\n"):
            changed = fixture.replace(original, replacement, 1)
            changed += "\n# " + path + "\nother:\n  paths:\n" + original
            with self.subTest(replacement=replacement):
                self.assertNotIn(path, pull_request_paths(changed))
        changed = fixture.replace(SELF_TEST, 'other.py', 1)
        self.assertNotIn(path, pull_request_paths(changed))

    def test_path_scalar_quoting_preserves_the_selected_identity(self):
        path = 'scripts/tests/' + SELF_TEST
        for scalar in (path, "'" + path + "'", '"' + path + '"'):
            with self.subTest(scalar=scalar):
                fixture = 'on:\n  pull_request:\n    paths:\n      - ' + scalar + '\n'
                self.assertEqual(pull_request_paths(fixture), [path])

    def test_toolchain_quoting_does_not_relax_the_qualified_pin(self):
        source = 'Install Rust\n        with:\n          toolchain: %s\n          components: clippy\n'
        for value in ('1.95.0', "'1.95.0'", '"1.95.0"'):
            with self.subTest(value=value):
                self.assertEqual(toolchain_inputs({'source': source % ('  ' + value)}),
                                 {'toolchain': '1.95.0', 'components': 'clippy'})
        self.assertNotEqual(toolchain_inputs({'source': source % '1.96.0'})['toolchain'], '1.95.0')

    def test_ambiguous_or_unsupported_selection_fields_refuse(self):
        prefix = 'on:\n  pull_request:\n'
        malformed = [prefix + '    paths: []\n',
                     prefix + '    paths:\n    paths:\n',
                     prefix + '    paths:\n      - *unresolved-path\n']
        for text in malformed:
            with self.subTest(text=text):
                with self.assertRaises(AssertionError):
                    pull_request_paths(text)
        duplicate = {'source': 'Install Rust\n        with:\n          components: clippy\n          components: clippy\n'}
        with self.assertRaises(AssertionError):
            toolchain_inputs(duplicate)

    def test_recording_model_rejects_unknown_conditions(self):
        with self.assertRaisesRegex(AssertionError, 'unexpected execution condition'):
            record_steps([{'name': 'unknown', 'if': 'always()'}])

    def test_literal_extractor_rejects_missing_duplicate_and_hidden_jobs(self):
        for text in ('', self.text + '\n  ' + JOB + ':\n    steps: []\n',
                     self.text.replace('  ' + JOB + ':\n', '  ' + JOB + ':\n    if: success()\n')):
            with self.subTest(text_length=len(text)):
                with self.assertRaises(AssertionError):
                    storage_steps(text)


if __name__ == '__main__':
    unittest.main()
