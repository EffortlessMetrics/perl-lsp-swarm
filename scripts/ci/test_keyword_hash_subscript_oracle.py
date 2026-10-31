"""False controls for the fixed, profile-bound Perl oracle slice (#14059)."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import keyword_hash_subscript_oracle as oracle

class OracleControls(unittest.TestCase):
    def test_matrix_integrity_and_false_controls(self):
        matrix, _ = oracle.load_matrix()
        self.assertEqual(44, len(matrix['cases']))
        mutations = []
        dropped = copy.deepcopy(matrix); dropped['cases'].pop(0); mutations.append(dropped)
        duplicated = copy.deepcopy(matrix); duplicated['cases'][0] = duplicated['cases'][1]; mutations.append(duplicated)
        stale = copy.deepcopy(matrix); stale['cases'][0]['source'] += ' '; mutations.append(stale)
        profile = copy.deepcopy(matrix); profile['profiles'][0]['version'] = '5.42.2'; mutations.append(profile)
        skipped = copy.deepcopy(matrix)
        for row in skipped['cases']:
            row['mismatch_owner'] = 14065
        mutations.append(skipped)
        substituted = copy.deepcopy(matrix)
        terminal = next(r for r in substituted['cases'] if r['id'] == 'last-scalar-terminal')
        comma = next(r for r in substituted['cases'] if r['id'] == 'last-scalar-comma')
        for field in ('source', 'source_sha256', 'subject_span', 'expected', 'oracle'):
            comma[field] = terminal[field]
        mutations.append(substituted)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'matrix.json'
            for mutant in mutations:
                path.write_text(json.dumps(mutant))
                with self.assertRaises(ValueError):
                    oracle.load_matrix(path)

    def test_instrument_failure_and_partial_rejection_are_not_agreement(self):
        matrix, _ = oracle.load_matrix()
        row = matrix['cases'][0]
        compiled = {'status': 'completed', 'exit': 0, 'stdout': '', 'stderr': ''}
        deparsed = dict(compiled, stdout=row['oracle']['deparse_stdout'])
        self.assertEqual('PROVEN', oracle.assess(row, compiled, deparsed)[0])
        for status in ['spawn_error', 'timeout', 'output_limit', 'signal']:
            self.assertEqual('NOT_PROVEN', oracle.assess(row, dict(compiled, status=status), deparsed)[0])
        # Even exact partial Deparse stdout cannot hide a Perl rejection.
        self.assertEqual('ORACLE_MISMATCH', oracle.assess(row, dict(compiled, exit=255), dict(deparsed, exit=255))[0])
        self.assertEqual('ORACLE_MISMATCH', oracle.assess(row, compiled, dict(deparsed, stdout=''))[0])
        missing = oracle.run('/missing/14059-perl')
        self.assertEqual('NOT_PROVEN', missing['status'])
        self.assertEqual([], missing['rows'])

    def test_other_runtime_is_not_this_profile(self):
        for version, archname in [('5.42.2', 'test'), ('5.40.1', 'deliberately-other-platform')]:
            identity = {'status': 'completed', 'exit': 0, 'stdout': f"version='{version}';\narchname='{archname}';\n", 'stderr': ''}
            with patch.object(oracle.shutil, 'which', return_value=sys.executable), \
                    patch.object(oracle, 'execute', return_value=identity):
                result = oracle.run('perl')
            self.assertEqual('NOT_PROVEN', result['status'])
            self.assertEqual([], result['rows'])

    def test_retained_receipt_matches_current_matrix(self):
        matrix, matrix_hash = oracle.load_matrix()
        receipt = json.loads(
            (oracle.ROOT / '.ci/keyword-hash-subscript/perl-5.40.1.json').read_text())
        self.assertEqual('PROVEN', receipt['status'])
        self.assertEqual(matrix_hash, receipt['matrix_sha256'])
        by_id = {r['id']: r for r in matrix['cases']}
        self.assertEqual(set(by_id), {r['id'] for r in receipt['rows']})
        for row in receipt['rows']:
            self.assertEqual(by_id[row['id']]['source_sha256'], row['source_sha256'])

if __name__ == '__main__':
    unittest.main()
