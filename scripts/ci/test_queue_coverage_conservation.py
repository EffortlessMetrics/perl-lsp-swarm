"""Independent regression oracle for #19326's coverage-conservation contract.

Run: python3 -B -m unittest scripts.ci.test_queue_coverage_conservation -v

The observation assertions below are test support, not a controller, producer,
receipt schema, or trusted finalizer. The six scalar identities come from the
existing policy; matrix observations are explicitly synthetic future-producer
controls. Passing these fixtures establishes no hosted execution or admission.
The catalog tests exercise the existing diagnostic and preserve its NOT_PROVEN
boundary for executable helpers. No workflow or production authority is added.
"""

from collections import Counter
import copy
from pathlib import Path
import subprocess
import tomllib
import unittest

from scripts.ci import validate_gate_enforcement_contract as catalog
from scripts.ci import test_validate_gate_enforcement_contract as catalog_fixture


ROOT = Path(__file__).resolve().parents[2]
# Acceptance names from #19326, independently pinned against the policy under
# test. Do not derive this set from that policy: simultaneous deletion would pass.
OBLIGATIONS = frozenset({
    'Compile All Targets (bit-rot guard)', 'Conflict marker check',
    'validate-title', 'Perl LSP Rust Small Result', 'ripr+ New Gap Gate',
    'Gate Enforcement Contract',
})


def mandatory_rows():
    policy = tomllib.loads((ROOT / '.ci/policies/required-checks.toml').read_text())
    return [row for row in policy['checks'] if row['name'] in OBLIGATIONS]


def assert_observations(case, expected, observed, *, head, runs, complete):
    """Assert an independently supplied, fully enumerated proof population.

    Keys are (workflow path, job ID, matrix member). The empty member is scalar.
    `runs` is the test's expected workflow-ID/run-ID/current-attempt binding,
    never inferred from observed successes. Input rows are already scoped to
    requested proofs; unrelated API jobs must be classified by the real owner.
    """
    case.assertIs(complete, True, 'incomplete observation pagination')
    case.assertTrue(expected, 'empty expected denominator')
    case.assertEqual(len(set(expected)), len(expected), 'duplicate expected member')
    case.assertEqual(Counter(row['key'] for row in observed), Counter(expected),
                     'missing, duplicate, or foreign proof member')
    ids = []
    for row in observed:
        case.assertIs(type(row['id']), int, 'job ID must be an integer')
        case.assertGreater(row['id'], 0)
        ids.append(row['id'])
        case.assertEqual(row['head_sha'], head, 'wrong integration subject')
        case.assertEqual(row['event'], 'merge_group', 'not actual G evidence')
        case.assertEqual(row['status'], 'completed', 'unfinished member')
        case.assertEqual(row['conclusion'], 'success', 'unsuccessful member')
        for field, value in zip(('workflow_id', 'run_id', 'run_attempt'), runs[row['key'][0]]):
            case.assertIs(type(row[field]), int, f'{field} must be an integer')
            case.assertGreater(row[field], 0)
            case.assertEqual(row[field], value, f'wrong {field}')
    case.assertEqual(len(set(ids)), len(ids), 'job ID reused for distinct members')


def synthetic_population():
    """Real scalar source identities plus a clearly invented two-member matrix."""
    expected = [(row['workflow'], row['job'], '') for row in mandatory_rows()]
    expected += [('.github/workflows/synthetic-g.yml', 'proof', member)
                 for member in ('linux', 'windows')]
    runs = {path: (100 + index, 200 + index, 2)
            for index, path in enumerate(sorted({key[0] for key in expected}))}
    observed = [dict(key=key, id=1000 + index, head_sha='a' * 40,
                     event='merge_group', status='completed', conclusion='success',
                     workflow_id=runs[key[0]][0], run_id=runs[key[0]][1], run_attempt=2)
                for index, key in enumerate(expected)]
    return expected, observed, runs


class ObservationOracleTests(unittest.TestCase):
    def setUp(self):
        self.expected, self.rows, self.runs = synthetic_population()
        self.assertEqual(len(self.expected), 8, 'six mandatory scalar + two synthetic members')

    def check(self, rows, *, expected=None, complete=True):
        assert_observations(self, self.expected if expected is None else expected,
                            rows, head='a' * 40, runs=self.runs, complete=complete)

    def test_complete_current_attempt_and_order_independence(self):
        self.check(self.rows)
        self.check(list(reversed(self.rows)))

    def test_each_missing_member_refuses_including_matrix(self):
        for index in range(len(self.rows)):
            with self.subTest(member=self.expected[index]), self.assertRaisesRegex(AssertionError, 'proof member'):
                self.check(self.rows[:index] + self.rows[index + 1:])

    def test_duplicate_and_equal_count_substitution_refuse(self):
        for index in range(len(self.rows)):
            with self.subTest(member=self.expected[index]):
                duplicate = copy.deepcopy(self.rows[index])
                duplicate['id'] = 9999
                with self.assertRaisesRegex(AssertionError, 'proof member'):
                    self.check(self.rows + [duplicate])
                replaced = copy.deepcopy(self.rows)
                replaced[(index + 1) % len(replaced)] = duplicate
                with self.assertRaisesRegex(AssertionError, 'proof member'):
                    self.check(replaced)

    def test_each_unsuccessful_member_refuses_even_if_aggregate_succeeded(self):
        for index in range(len(self.rows)):
            for conclusion in ('skipped', 'cancelled', 'failure', 'timed_out', 'neutral', None):
                with self.subTest(member=self.expected[index], conclusion=conclusion):
                    rows = copy.deepcopy(self.rows)
                    rows[index]['conclusion'] = conclusion
                    with self.assertRaisesRegex(AssertionError, 'unsuccessful member'):
                        self.check(rows)

    def test_wrong_subject_workflow_run_attempt_and_event_refuse(self):
        changes = {'head_sha': 'b' * 40, 'workflow_id': 999, 'run_id': 999,
                   'run_attempt': 1, 'event': 'pull_request', 'status': 'in_progress'}
        for index in range(len(self.rows)):
            for field, value in changes.items():
                with self.subTest(member=self.expected[index], field=field):
                    rows = copy.deepcopy(self.rows)
                    rows[index][field] = value
                    with self.assertRaises(AssertionError):
                        self.check(rows)

    def test_all_green_stale_attempt_cannot_supply_current_denominator(self):
        rows = copy.deepcopy(self.rows)
        for row in rows:
            row['run_attempt'] = 1
        with self.assertRaisesRegex(AssertionError, 'wrong run_attempt'):
            self.check(rows)

    def test_latest_success_per_member_cannot_mix_attempts(self):
        rows = copy.deepcopy(self.rows)
        rows[-1]['run_attempt'] = 1
        self.assertTrue(all(row['conclusion'] == 'success' for row in rows))
        with self.assertRaisesRegex(AssertionError, 'wrong run_attempt'):
            self.check(rows)

    def test_non_integer_or_nonpositive_identity_refuses(self):
        for field in ('id', 'workflow_id', 'run_id', 'run_attempt'):
            for value in (True, False, '2', None, 0, -1):
                with self.subTest(field=field, value=value):
                    rows = copy.deepcopy(self.rows)
                    rows[-1][field] = value
                    with self.assertRaises(AssertionError):
                        self.check(rows)

    def test_reused_job_id_cannot_cover_distinct_matrix_members(self):
        rows = copy.deepcopy(self.rows)
        rows[-1]['id'] = rows[-2]['id']
        with self.assertRaisesRegex(AssertionError, 'job ID reused'):
            self.check(rows)

    def test_partial_pages_empty_or_duplicate_plan_refuse(self):
        for value in (False, None, 1, 'true'):
            with self.subTest(complete=value), self.assertRaisesRegex(AssertionError, 'pagination'):
                self.check(self.rows, complete=value)
        with self.assertRaisesRegex(AssertionError, 'empty expected'):
            self.check([], expected=[])
        with self.assertRaisesRegex(AssertionError, 'duplicate expected'):
            self.check(self.rows, expected=self.expected + [self.expected[-1]])

    def test_aggregate_count_and_success_only_mutants_are_discriminated(self):
        # Wrong implementations can see identical successful counts after a
        # duplicate replaces a missing member, or after stale evidence is mixed.
        for mutation in ('duplicate', 'cross-attempt'):
            rows = copy.deepcopy(self.rows)
            if mutation == 'duplicate':
                rows[-1] = copy.deepcopy(rows[-2])
            else:
                rows[-1]['run_attempt'] = 1
            wrong_accepts = (len(rows) == len(self.expected)
                             and all(row['conclusion'] == 'success' for row in rows))
            self.assertTrue(wrong_accepts)
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                self.check(rows)


class SourceBoundaryTests(unittest.TestCase):
    def test_six_obligations_resolve_in_complete_tracked_catalog(self):
        workflows = catalog.read_workflow_catalog(ROOT)
        tracked = subprocess.check_output(['git', 'ls-files', '-z', '--', '.github/workflows'],
                                          cwd=ROOT).decode().split('\0')
        expected = {path for path in tracked if Path(path).suffix in ('.yml', '.yaml')}
        self.assertTrue(expected)
        self.assertEqual(set(workflows), expected)
        rows = mandatory_rows()
        self.assertEqual(Counter(row['name'] for row in rows), Counter(OBLIGATIONS))
        for row in rows:
            with self.subTest(context=row['name']):
                self.assertTrue(row['required'])
                self.assertEqual(row['policy_role'], 'required')
                workflow = workflows[row['workflow']]
                self.assertIn(row['job'], workflow.jobs)
                self.assertEqual(workflow.jobs[row['job']].name, row['name'])
                self.assertIn('merge_group', workflow.events)

    def test_helper_dispatch_static_success_is_never_runtime_acceptance(self):
        fixture = catalog_fixture.QualificationCallerTests()
        code, result = fixture.run_catalog({'helper.yml':
            'on: push\njobs:\n  launch:\n    runs-on: ubuntu-latest\n'
            '    steps:\n      - run: bash scripts/start-product-ci.sh\n'})
        self.assertIn(code, (0, 1))
        self.assertEqual(result['qualification_status'], 'NOT_PROVEN')
        self.assertNotEqual(result.get('qualification_result'), 'SUCCESS')

    def test_indirect_reusable_and_result_starts_require_separate_assessment(self):
        fixture = catalog_fixture.QualificationCallerTests()
        code, result = fixture.run_catalog({
            'wrapper.yml': fixture.caller('middle.yml', 'push'),
            'middle.yml': fixture.caller('ci.yml', 'workflow_call'),
            'result.yml': 'on: workflow_run\njobs:\n  launch:\n    steps: []\n',
        })
        self.assertEqual(code, 1)
        callers = {row['subject'] for row in result['findings']
                   if row['code'] == 'qualification_unresolved_caller'}
        self.assertEqual(callers, {'.github/workflows/wrapper.yml', '.github/workflows/middle.yml'})
        self.assertIn('qualification_unresolved_result_path', {row['code'] for row in result['findings']})
        self.assertEqual(result['qualification_status'], 'NOT_PROVEN')


if __name__ == '__main__':
    unittest.main()
