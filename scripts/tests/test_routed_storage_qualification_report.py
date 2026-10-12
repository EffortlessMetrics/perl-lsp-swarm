import importlib.util
import json
import copy
import socket
import os
import subprocess
import sys
import textwrap
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('report', Path(__file__).resolve().parents[1] / 'ci/report_routed_storage_qualification.py')
subject = importlib.util.module_from_spec(spec); spec.loader.exec_module(subject)


class QualificationInventory(unittest.TestCase):
    # All owner/kernel records here are explicit doubles, never native closure proof.
    def metadata(self, root='/work'):
        names = [*subject.PACKAGES, 'routed-test-cohorts-bootstrap', 'unselected']
        packages = [{'id': self.package_id(name, root), 'name': name} for name in names]
        return {'packages': packages, 'workspace_members': [item['id'] for item in packages], 'workspace_root': root}

    def package_id(self, package, root='/work'):
        return 'path+file://' + root + '/' + ('xtask' if package == 'xtask' else 'crates/' + package) + '#0.17.0'

    def owner_log(self, evidence, operation='test', cancelled=False):
        paths = {key: str(evidence / key) for key in ('target', 'build', 'cargo_home', 'temp')}
        package_args = [item for package in subject.PACKAGES for item in ('-p', package)]
        argv = (['build', '-p', 'perllsp', '--locked'] if operation == 'build' else
                ['run', '--locked', '-p', 'xtask', '--bin', 'routed-test-cohorts', '--', *package_args, '--evidence-dir', str(evidence / 'cohort')] if operation == 'run' else
                ['test', '--locked', '--tests', *package_args, '--message-format=json-render-diagnostics'])
        descriptor = {'worktree': '/work', 'resources': paths,
                      'scope': {'operation': operation, 'cargo_argv_sha256': subject.argv_digest(argv), 'resources': paths, 'hostname': socket.gethostname(), 'worktree': '/work', 'working_directory': '/work'},
                      'lease': str(evidence / 'lease'), 'lease_identity': [1, 2],
                      'lease_marker': str(evidence / 'lease/owner'), 'marker_identity': [1, 3]}
        settlement = {'tree_settled': True, 'proof': 'kernel ECHILD (__WALL)', 'errors': [], 'pending_bound_pids': [], 'lease_released': True, 'cancelled': cancelled,
                      **{key: descriptor[key] for key in ('lease', 'lease_identity', 'lease_marker', 'marker_identity')}}
        (evidence / 'resources.json').write_text(json.dumps(paths))
        (evidence / 'cargo-metadata.json').write_text(json.dumps(self.metadata()))
        return 'cargo-admitted resources: ' + json.dumps(descriptor) + '\n' + ('' if cancelled else 'cargo-admitted Cargo product: ' + json.dumps({'lease': descriptor['lease'], 'exit_code': 0}) + '\n') + 'cargo-admitted Cargo settlement: ' + json.dumps(settlement) + '\n'

    def test_two_complete_producer_formats_join_and_missing_package_does_not(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            (evidence / 'head.txt').write_text('candidate')
            storage = [{'kind': 'retained', 'valid': True, 'elapsed_seconds': 0,
                        'filesystems': {}, 'allocated_bytes': 0}, {'kind': 'summary', 'command_status': 0}]
            (evidence / 'storage.jsonl').write_text('\n'.join(json.dumps(row) for row in storage))
            rows = []
            for package in subject.PACKAGES:
                row = self.row()
                row['package_id'] = self.package_id(package)
                row['target']['src_path'] = '/work/' + ('xtask' if package == 'xtask' else 'crates/' + package) + '/tests/shared.rs'
                row['executable'] = '/private/' + package
                rows.append(row)
            compiler = '\n'.join(json.dumps(row) for row in [*rows, {'reason': 'build-finished', 'success': True}])
            (evidence / 'route.stdout').write_text(compiler + '\n' + ('running 4 tests\ntest case ... ok\ntest case ... ok\ntest skipped ... ignored\ntest skipped ... ignored\ntest result: ok. 2 passed; 0 failed; 2 ignored\n' * 6))
            (evidence / 'route.stderr').write_text(''.join('Running tests/shared.rs (' + row['executable'] + ')\n' for row in rows)
                + self.owner_log(evidence))
            ordinary = subject.report(evidence, 'ordinary', Path('/work'))
            self.assertTrue(ordinary['case_inventory_complete'])
            cohort = evidence / 'cohort'; cohort.mkdir()
            for index, producer in enumerate((['cargo', 'check', '--tests'], ['cargo', 'test', '--no-run'])):
                stem = f'{index:04}'
                (cohort / (stem + '-request.json')).write_text(json.dumps({'producer': producer, 'evidence_path': '/host/' + stem + '-compiler.jsonl'}))
                (cohort / (stem + '-compiler.jsonl')).write_text(compiler)
                receipt = {'status': 0, 'settlement': {'tree_settled': True, 'proof': 'kernel ECHILD (__WALL)', 'errors': [], 'pending_bound_pids': []}, 'coverage': []}
                if index:
                    receipt['coverage'] = [{'executable': row['executable'], 'cases': ['case', 'case', 'skipped', 'skipped'], 'ignored': ['skipped', 'skipped']} for row in rows]
                (cohort / (stem + '-result.json')).write_text(json.dumps(receipt))
            (evidence / 'route.stderr').write_text(self.owner_log(evidence, 'run'))
            native = subject.report(evidence, 'cohort', Path('/work'))
            self.assertTrue(native['case_inventory_complete'])
            self.assertEqual((native['case_count'], native['ignored_count']), (24, 12))
            self.assertEqual(native['units_sha256'], ordinary['units_sha256'])
            self.assertEqual(native['cases_sha256'], ordinary['cases_sha256'])
            result_path = cohort / '0001-result.json'
            valid_receipt = result_path.read_text()
            overflow = json.loads(valid_receipt)
            overflow['coverage'][0]['ignored'].append('skipped')
            result_path.write_text(json.dumps(overflow))
            invalid_population = subject.report(evidence, 'cohort', Path('/work'))
            self.assertFalse(invalid_population['case_inventory_complete'])
            self.assertFalse(invalid_population['case_receipts_successful_and_settled'])
            self.assertEqual(len(invalid_population['receipt_errors']), 1)
            result_path.write_text(valid_receipt)
            for field, value in [('pending_bound_pids', [91]), ('errors', ['unproven']), ('proof', 'boolean')]:
                invalid = json.loads(valid_receipt); invalid['settlement'][field] = value
                result_path.write_text(json.dumps(invalid))
                partial = subject.report(evidence, 'cohort', Path('/work'))
                self.assertEqual(partial['case_count'], 24)
                self.assertFalse(partial['case_receipts_successful_and_settled'])
                self.assertFalse(partial['case_inventory_complete'])
            result_path.write_text(valid_receipt)
            for status, settled in [(101, True), (0, False)]:
                invalid = json.loads(result_path.read_text())
                invalid.update(status=status, settlement={'tree_settled': settled})
                result_path.write_text(json.dumps(invalid))
                failed = subject.report(evidence, 'cohort', Path('/work'))
                self.assertTrue(failed['observed_case_inventory_complete'])
                self.assertFalse(failed['case_inventory_complete'])
                self.assertFalse(failed['case_receipts_successful_and_settled'])
                self.assertEqual(failed['case_count'], 24)
            (cohort / '0000-compiler.jsonl').write_text('\n'.join(json.dumps(row) for row in [*rows[:-1], {'reason': 'build-finished', 'success': True}]))
            self.assertFalse(subject.report(evidence, 'cohort', Path('/work'))['unit_inventory_complete'])

    def test_interrupted_result_preserves_partial_metrics_without_verified_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); cohort = evidence / 'cohort'; cohort.mkdir()
            (evidence / 'head.txt').write_text('candidate')
            (evidence / 'storage.jsonl').write_text('\n'.join(json.dumps(row) for row in [
                {'kind': 'retained', 'valid': True, 'elapsed_seconds': 0, 'filesystems': {},
                 'allocated_bytes': 4096, 'allocated_by_root': [{'root': '/private', 'allocated_bytes': 4096}]},
                {'kind': 'summary', 'command_status': 130, 'sampled_peak_allocated_bytes': 8192}]))
            (evidence / 'route.stderr').write_text(self.owner_log(evidence, 'run'))
            rows = []
            for package in subject.PACKAGES:
                row = self.row()
                row['package_id'] = self.package_id(package)
                row['target']['src_path'] = '/work/' + ('xtask' if package == 'xtask' else 'crates/' + package) + '/tests/shared.rs'
                row['executable'] = '/private/' + package; rows.append(row)
            for index, selected in [(0, rows), (1, rows[:3]), (2, rows[3:])]:
                stem = f'{index:04}'
                producer = ['cargo', 'check', '--tests'] if index == 0 else ['cargo', 'test', '--no-run']
                (cohort / (stem + '-request.json')).write_text(json.dumps({'producer': producer, 'evidence_path': '/host/' + stem + '-compiler.jsonl'}))
                (cohort / (stem + '-compiler.jsonl')).write_text('\n'.join(json.dumps(row) for row in [*selected, {'reason': 'build-finished', 'success': True}]))
                coverage = [] if index == 0 else [{'executable': row['executable'], 'cases': ['case'], 'ignored': []} for row in selected]
                (cohort / (stem + '-result.json')).write_text(json.dumps({'status': 0, 'settlement': {'tree_settled': True, 'proof': 'kernel ECHILD (__WALL)', 'errors': [], 'pending_bound_pids': []}, 'coverage': coverage}))
            interrupted = cohort / '0002-result.json'
            good = interrupted.read_text()
            for raw in [None, '', '{', 'null', '[]', '{"status":false}',
                        '{"status":0,"settlement":{"tree_settled":true},"coverage":[null]}']:
                if raw is None:
                    interrupted.unlink(missing_ok=True)
                else:
                    interrupted.write_text(raw)
                partial = subject.report(evidence, 'cohort', Path('/work'))
                self.assertEqual((partial['unit_count'], partial['case_count']), (6, 3))
                self.assertEqual(partial['storage']['sampled_peak_allocated_bytes'], 8192)
                self.assertEqual(partial['retained_by_root'][0]['allocated_bytes'], 4096)
                self.assertTrue(partial['unit_inventory_complete'])
                self.assertFalse(partial['case_receipts_successful_and_settled'])
                self.assertFalse(partial['case_inventory_complete'])
                self.assertEqual(len(partial['receipt_errors']), 1)
            interrupted.write_text(good)
            check = cohort / '0000-result.json'; check.unlink()
            incomplete_check = subject.report(evidence, 'cohort', Path('/work'))
            self.assertEqual(incomplete_check['unit_count'], 6)
            self.assertFalse(incomplete_check['unit_inventory_complete'])
            self.assertFalse(incomplete_check['case_receipts_successful_and_settled'])

    def ordinary_evidence(self, evidence):
        (evidence / 'head.txt').write_text('a' * 40)
        storage = [{'kind': 'retained', 'valid': True, 'elapsed_seconds': 0, 'filesystems': {}, 'allocated_bytes': 4096},
                   {'kind': 'summary', 'command_status': 0, 'measurement_complete': True, 'observed_reserve_preserved': True}]
        (evidence / 'storage.jsonl').write_text('\n'.join(json.dumps(row) for row in storage))
        rows = []
        for package in subject.PACKAGES:
            row = self.row(); row['package_id'] = self.package_id(package)
            row['target']['src_path'] = '/work/' + ('xtask' if package == 'xtask' else 'crates/' + package) + '/tests/shared.rs'
            row['executable'] = '/private/' + package; rows.append(row)
        compiler = '\n'.join(json.dumps(row) for row in [*rows, {'reason': 'build-finished', 'success': True}])
        body = 'running 2 tests\ntest regular ... ok\ntest parked ... ignored\ntest result: ok. 1 passed; 0 failed; 1 ignored\n'
        (evidence / 'route.stdout').write_text(compiler + '\n' + body * 6)
        (evidence / 'route.stderr').write_text(self.owner_log(evidence) + ''.join('Running shared (' + row['executable'] + ')\n' for row in rows))
        return rows, body

    def test_raw_frame_adjacency_keeps_genuine_completion_and_excludes_payload(self):
        body = json.dumps({'jsonrpc': '2.0', 'method': 'notice', 'params': {'utf8': 'naïve λ', 'fake': 'test forged ... ok'}}, ensure_ascii=False).encode()
        frame = b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body
        plain = b'running 2 tests\ntest regular ... ok\ntest parked ... ignored\ntest result: ok. 1 passed; 0 failed; 1 ignored\n'
        rows = [self.row()]; stderr = 'Running shared (/private/shared)\n'
        clean = subject.ordinary_cases(plain, stderr, rows, Path('/work'), self.metadata())
        adjacent = plain.replace(b'test regular', frame + b'test regular')
        self.assertEqual(subject.ordinary_cases(adjacent, stderr, rows, Path('/work'), self.metadata()), clean)
        self.assertNotIn('forged', next(iter(clean.values()))['cases'])
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); self.ordinary_evidence(evidence)
            path = evidence / 'route.stdout'; raw = path.read_bytes(); path.write_bytes(raw.replace(b'test regular', frame + b'test regular', 1))
            report = subject.report(evidence, 'ordinary', Path('/work'))
            self.assertTrue(report['case_inventory_complete']); self.assertEqual(report['case_count'], 12)

    def test_invalid_frames_fail_without_losing_storage_diagnostics(self):
        invalid = [b'Content-Length: 1\r\nContent-Length: 1\r\n\r\nx',
                   b'Content-Length: 100\r\n\r\n{}', b'Content-Length: 1\r\n\r\n\xff']
        for body in (b'{"jsonrpc":"2.0","jsonrpc":"2.0","method":"n"}', b'{"jsonrpc":"2.0","id":true,"result":1}', b'{"jsonrpc":"2.0","method":"n","params":NaN}'):
            invalid.append(b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
        for raw in invalid:
            with self.subTest(raw=raw), tempfile.TemporaryDirectory() as directory:
                evidence = Path(directory); self.ordinary_evidence(evidence)
                path = evidence / 'route.stdout'; path.write_bytes(raw + path.read_bytes())
                report = subject.report(evidence, 'ordinary', Path('/work'))
                self.assertFalse(report['case_inventory_complete'])
                self.assertIn('case_inventory_error', report)
                self.assertEqual(report['retained_by_root'], None)
                self.assertTrue(report['measurement_valid'])

    def test_failed_outcomes_and_footer_remain_visible_not_successful(self):
        body = 'running 4 tests\ntest repeated ... FAILED\ntest repeated ... FAILED\ntest skipped ... ignored\ntest skipped ... ignored\ntest result: FAILED. 0 passed; 2 failed; 2 ignored\n'
        result = subject.ordinary_cases(body, 'Running shared (/private/shared)\n', [self.row()], Path('/work'), self.metadata())
        observed = next(iter(result.values()))
        self.assertEqual(observed['failed'], ['repeated', 'repeated'])
        self.assertEqual(observed['ignored'], ['skipped', 'skipped'])
        self.assertEqual(observed['footer_status'], 'FAILED')
        self.assertEqual(observed['footer_counts'], {'passed': 0, 'failed': 2, 'ignored': 2})
        self.assertTrue(observed['footer_consistent'])
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); self.ordinary_evidence(evidence)
            path = evidence / 'route.stdout'; path.write_text(path.read_text().replace('regular ... ok', 'regular ... FAILED', 1))
            report = subject.report(evidence, 'ordinary', Path('/work'))
            self.assertTrue(report['observed_case_inventory_complete'])
            self.assertFalse(report['case_execution_successful'])
            self.assertFalse(report['case_inventory_complete'])

    def test_package_id_overrides_shared_source_layout_and_missing_metadata_is_unproven(self):
        row = self.row(); row['package_id'] = self.package_id('routed-test-cohorts-bootstrap')
        row['target']['src_path'] = '/work/xtask/src/bin/routed-test-cohorts.rs'
        with self.assertRaisesRegex(ValueError, 'routed-test-cohorts-bootstrap'):
            subject.units([row], Path('/work'), self.metadata())
        observed = subject.units([row], Path('/work'))
        self.assertEqual(json.loads(next(iter(observed)))[0], row['package_id'])
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); self.ordinary_evidence(evidence)
            (evidence / 'cargo-metadata.json').unlink()
            report = subject.report(evidence, 'ordinary', Path('/work'))
            self.assertEqual((report['unit_count'], report['case_count']), (6, 12))
            self.assertFalse(report['package_inventory_verified'])
            self.assertFalse(report['case_inventory_complete'])
            self.assertEqual(report['qualification_status'], 'NOT_PROVEN')

    def test_terminal_owner_binding_refuses_prior_build_foreign_or_unsettled_episode(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            text = self.owner_log(evidence)
            binding = lambda value: subject.terminal_owner_binding(value, 'ordinary', Path('/work'), evidence / 'cohort', json.loads((evidence / 'resources.json').read_text()))
            self.assertTrue(binding(text)['verified'])
            prior = self.owner_log(evidence, 'build')
            self.assertFalse(binding(prior)['verified'])
            descriptor, product, receipt = [json.loads(line.split(': ', 1)[1]) for line in text.splitlines()]
            for field, value in (('lease_identity', [9, 9]), ('pending_bound_pids', [99]), ('errors', ['unknown']), ('proof', 'boolean assertion'), ('lease_released', False)):
                with self.subTest(field=field):
                    changed = dict(receipt, **{field: value})
                    raw = text.rsplit('cargo-admitted Cargo settlement: ', 1)[0] + 'cargo-admitted Cargo settlement: ' + json.dumps(changed)
                    self.assertFalse(binding(raw)['verified'])
            for field, value in (('operation', 'build'), ('cargo_argv_sha256', '0' * 64), ('worktree', '/foreign')):
                changed = copy.deepcopy(descriptor); changed['scope'][field] = value
                raw = 'cargo-admitted resources: ' + json.dumps(changed) + '\n' + '\n'.join(text.splitlines()[1:])
                self.assertFalse(binding(raw)['verified'])
            self.assertFalse(binding(text + 'cargo-admitted Cargo settlement: ' + json.dumps(receipt))['verified'])
            self.assertFalse(binding(prior + text.splitlines()[0] + '\n')['verified'])
            self.assertTrue(binding(prior + text)['verified'])
            missing_prior = '\n'.join(prior.splitlines()[:-1]) + '\n'
            self.assertFalse(binding(missing_prior + text)['verified'])

    def test_timeout_retry_requires_actual_cancelled_last_started_owner_and_current_absence(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); self.ordinary_evidence(evidence)
            (evidence / 'status.txt').write_text('124')
            summary = {'kind': 'summary', 'command_status': 124, 'measurement_complete': True, 'observed_reserve_preserved': True}
            (evidence / 'storage.jsonl').write_text(json.dumps(summary))
            retry = lambda: subject.retry_eligible(evidence, 'ordinary', Path('/work'), 'a' * 40)
            for operation in ('test', 'build'):
                (evidence / 'route.stderr').write_text(self.owner_log(evidence, operation, cancelled=True))
                self.assertTrue(retry())
            (evidence / 'route.stderr').write_text(self.owner_log(evidence, 'build'))
            self.assertFalse(retry())
            prior = self.owner_log(evidence, 'build', cancelled=True)
            current = self.owner_log(evidence, 'test', cancelled=True)
            (evidence / 'route.stderr').write_text(prior + current.splitlines()[0] + '\n')
            self.assertFalse(retry())
            (evidence / 'route.stderr').write_text(current)
            (evidence / 'lease').mkdir(); (evidence / 'lease/owner').touch()
            self.assertFalse(retry())
            (evidence / 'lease/owner').unlink(); (evidence / 'lease').rmdir()
            for field in ('measurement_complete', 'observed_reserve_preserved'):
                (evidence / 'storage.jsonl').write_text(json.dumps({**summary, field: False}))
                self.assertFalse(retry())
            (evidence / 'storage.jsonl').write_text(json.dumps({'kind': 'periodic', 'valid': False}) + '\n' + json.dumps(summary))
            self.assertFalse(retry())
            (evidence / 'storage.jsonl').write_text(json.dumps({**summary, 'worker_failure': {'error_type': 'MemoryError'}}))
            self.assertFalse(retry())
            (evidence / 'storage.jsonl').write_text(json.dumps(summary))
            self.assertFalse(subject.retry_eligible(evidence, 'ordinary', Path('/work'), 'b' * 40))

    def test_qualified_source_attempt_is_not_invented_from_head_file(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory); self.ordinary_evidence(evidence)
            report = subject.report(evidence, 'ordinary', Path('/work'), 'b' * 40)
            self.assertFalse(report['candidate_head_matches_caller'])
            self.assertFalse(report['case_inventory_complete'])
            self.assertEqual(report['qualification_status'], 'NOT_PROVEN')
            self.assertIn('candidate_source_attempt_attestation', report['qualification_missing_authority'])

    def test_actual_workflow_retry_entrypoint_propagates_allow_and_refusal(self):
        repo = Path(__file__).resolve().parents[2]
        text = (repo / '.github/workflows/ci-gate-self-tests.yml').read_text()
        block = text.split('if [ "$status" = 124 ] && python3 - <<\'PY\'\n', 1)[1].split('\n          PY', 1)[0]
        block = textwrap.dedent(block)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); evidence = root / 'qualification-evidence'; evidence.mkdir()
            self.ordinary_evidence(evidence)
            (evidence / 'status.txt').write_text('124')
            (evidence / 'storage.jsonl').write_text(json.dumps({'kind': 'summary', 'command_status': 124, 'measurement_complete': True, 'observed_reserve_preserved': True}))
            text = self.owner_log(evidence, 'test', cancelled=True)
            descriptor = json.loads(text.splitlines()[0].split(': ', 1)[1])
            descriptor['worktree'] = str(root)
            descriptor['scope'].update(worktree=str(root), working_directory=str(root))
            text = 'cargo-admitted resources: ' + json.dumps(descriptor) + '\n' + '\n'.join(text.splitlines()[1:])
            (evidence / 'route.stderr').write_text(text)
            env = dict(os.environ, PYTHONPATH=str(repo), QUALIFICATION_MODE='ordinary', QUALIFICATION_CANDIDATE='a' * 40)
            def invoke():
                return subprocess.run([sys.executable, '-c', block], cwd=root, env=env, capture_output=True, text=True, timeout=10)
            accepted = invoke(); self.assertEqual(accepted.returncode, 0, accepted.stderr)
            (evidence / 'route.stderr').write_text(text.split('cargo-admitted Cargo settlement:', 1)[0])
            refused = invoke(); self.assertEqual(refused.returncode, 75, refused.stderr)
            (evidence / 'route.stderr').write_text(text)
            env['QUALIFICATION_CANDIDATE'] = 'b' * 40
            refused = invoke(); self.assertEqual(refused.returncode, 75, refused.stderr)

    def test_independent_workflow_controls_precede_native_checks_and_retry_uses_shared_binding(self):
        text = (Path(__file__).resolve().parents[2] / '.github/workflows/ci-gate-self-tests.yml').read_text()
        job = text.split('  cargo-admitted-storage-self-test:', 1)[1].split('  cargo-toolchain-guard-self-test:', 1)[0]
        first_native = job.index('../../../../cargo-admitted test')
        for name in ('test_storage_doctor.py', 'test_measure_cargo_storage.py', 'test_routed_storage_qualification_report.py'):
            self.assertLess(job.index(name), first_native)
        for command in (
            '../../../../cargo-admitted test --locked --offline -p cohort-b --test shared',
            '../../../../cargo-admitted test --locked --offline -p cohort-b --features enabled-bin --test runtime_env -- --nocapture',
            '../../../../cargo-admitted test --locked --offline -p cohort-b --features enabled-bin --tests',
            '../../../../cargo-admitted bench --locked --offline -p cohort-b --features enabled-bin --bench runtime_env',
            'scripts/cargo-admitted test --locked -p xtask --bin routed-test-cohorts',
            'scripts/cargo-admitted clippy -p xtask --all-targets --profile agent --locked -- -D warnings'):
            self.assertEqual(job.count(command), 1)
        self.assertNotIn('continue-on-error', job)
        campaign = text.split('  routed-storage-qualification:', 1)[1]
        self.assertIn('from scripts.ci.report_routed_storage_qualification import retry_eligible', campaign)
        self.assertNotIn("receipts = re.findall", campaign)
        self.assertIn('raise SystemExit(0 if allowed else 75)', campaign)

    def row(self, root='/work', features=None):
        return {'reason': 'compiler-artifact', 'package_id': self.package_id('perl-dap', root), 'target': {'src_path': root + '/crates/perl-dap/tests/shared.rs',
            'name': 'shared', 'kind': ['test']}, 'features': features or [],
            'profile': {'test': True, 'opt_level': '1'}, 'executable': '/private/shared'}

    def test_cross_host_roots_normalize_but_features_and_omissions_do_not(self):
        a = subject.units([self.row('/one')], Path('/one'), self.metadata('/one'))
        b = subject.units([self.row('/two')], Path('/two'), self.metadata('/two'))
        self.assertEqual(subject.digest(a), subject.digest(b))
        self.assertNotEqual(subject.digest(a), subject.digest(subject.units([self.row('/one', ['extra'])], Path('/one'), self.metadata('/one'))))
        self.assertNotEqual(subject.digest(a), subject.digest({}))

    def test_conflicting_unit_context_refuses(self):
        with self.assertRaisesRegex(ValueError, 'conflicting'):
            subject.units([self.row(), self.row(features=['extra'])], Path('/work'), self.metadata())

    def test_extra_package_is_not_silently_dropped(self):
        row = self.row()
        row['package_id'] = self.package_id('unselected')
        row['target']['src_path'] = '/work/crates/unselected/tests/shared.rs'
        with self.assertRaisesRegex(ValueError, 'unexpected package'):
            subject.units([row], Path('/work'), self.metadata())

    def test_actual_stdout_maps_cases_and_ignored_labels_to_compiler_target(self):
        stdout = 'running 2 tests\ntest panic - should panic ... ok\ntest skipped ... ignored, reason\ntest result: ok. 1 passed; 0 failed; 1 ignored\n'
        result = subject.ordinary_cases(stdout, '     Running tests/shared.rs (/private/shared)\n', [self.row()], Path('/work'), self.metadata())
        self.assertEqual(next(iter(result.values())), {'cases': ['panic', 'skipped'], 'ignored': ['skipped'], 'failed': [], 'outcomes': [('panic', 'ok'), ('skipped', 'ignored')], 'footer_status': 'ok', 'footer_counts': {'passed': 1, 'failed': 0, 'ignored': 1}, 'footer_consistent': True})
        changed = {key: {**value, 'ignored': []} for key, value in result.items()}
        self.assertNotEqual(subject.digest(result), subject.digest(changed))

    def test_omissions_excess_occurrences_and_ambiguous_nesting_refuse(self):
        for body in ('running 2 tests\ntest one ... ok\ntest result: ok.\n',
                     'running 1 test\ntest one ... ok\ntest one ... ok\ntest result: ok.\n',
                     'running 1 test\nrunning 1 test\n'):
            with self.assertRaises(ValueError):
                subject.ordinary_cases(body, 'Running tests/shared.rs (/private/shared)\n', [self.row()], Path('/work'), self.metadata())


if __name__ == '__main__':
    unittest.main()
