#!/usr/bin/env python3
"""Real xtask/Git integration controls; only the external RIPR analyzer is a fixture.

Run via xtask/tests/ripr_comparison_subject.rs so the binary is built from the
same candidate. No test runs an expensive analysis or claims native qualification.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import tomllib
ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(sys.argv.pop(1)).resolve()
GIT_ENV = subprocess.check_output(['git', 'rev-parse', '--local-env-vars'], text=True).split()
ENV = {k: v for k, v in os.environ.items() if k not in GIT_ENV and k != 'GIT_DEFAULT_HASH'}
ENV.update(RUST_BACKTRACE='0', RUST_LIB_BACKTRACE='0', GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1', GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid', GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')
ANALYZER = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args=sys.argv[1:]
with open(os.environ['ANALYZER_CALLS'],'a') as f: f.write(json.dumps(args)+'\n')
if args[0]=='review-comments':
    if os.environ.get('ANALYZER_FAIL_GUIDANCE'): raise SystemExit(1)
    out=pathlib.Path(args[args.index('--out')+1]); out.parent.mkdir(parents=True,exist_ok=True)
    packet=dict(schema_version='0.1',tool='ripr',base=args[args.index('--base')+1],head=args[args.index('--head')+1],status='advisory',comments=[],summary_only=[],suppressed=[],warnings=[],summary={})
    out.write_text(json.dumps(packet)); out.with_suffix('.md').write_text('Fixture guidance\n')
else:
    if os.environ.get('ANALYZER_DIRTY_SOURCE'): pathlib.Path('src/lib.rs').write_text('pub fn changed_during_analysis() {}\n')
    findings=[];summary=dict(no_static_path=0,reachable_unrevealed=0,weakly_exposed=0)
    if os.environ.get('ANALYZER_GAP'):
        findings=[dict(id='probe:integration',classification='no_static_path',probe=dict(id='probe:integration',family='error_path',file='src/lib.rs',line=3,expression='4'),ripr=dict(reach=dict(state='no',summary='Fixture integration gap')))]
        summary=dict(no_static_path=1,reachable_unrevealed=0,weakly_exposed=0)
    print(json.dumps(dict(base=os.environ.get('ANALYZER_FORCE_BASE') or (args[args.index('--base')+1] if '--base' in args else os.environ.get('ANALYZER_BASE')),summary=summary,findings=findings)))
'''


class Comparison(unittest.TestCase):

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / 'repo'
        self.repo.mkdir()
        self.env = dict(ENV)
        self.git('init', '--template=', '--initial-branch=main')
        self.write('Cargo.toml', '[workspace]\nmembers=["."]\n[workspace.package]\nrepository="https://github.com/EffortlessMetrics/perl-lsp"\n[package]\nname="scope-fixture"\nversion="0.1.0"\nedition="2021"\n')
        repository = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['repository']
        self.write('Cargo.toml', (self.repo / 'Cargo.toml').read_text().replace('https://github.com/EffortlessMetrics/perl-lsp', repository))
        self.write('xtask/Cargo.toml', '[package]\nname="xtask"\nversion="0.1.0"\n')
        self.write('.gitignore', 'target/\nCargo.lock\n')
        self.write('src/lib.rs', 'pub fn mainline() -> u8 { 1 }\n')
        self.write('scripts/change.py', 'value = 1\n')
        self.write('policy/ripr-suppressions.toml', (ROOT / 'policy/ripr-suppressions.toml').read_text())
        self.write('exceptions.toml', 'schema_version = 1\npolicy = "quality-gate-exceptions"\nowner = "test"\nstatus = "active"\nupdated = "2026-01-01"\ndue_review = "warn"\n')
        self.A = self.commit('base')
        self.git('checkout', '-b', 'feature')
        self.write('scripts/change.py', 'value = 2\n')
        self.H = self.commit('candidate')
        self.git('checkout', 'main')
        self.write('src/lib.rs', 'pub fn mainline() -> u8 { 3 }\n')
        self.P = self.commit('unrelated Rust advance')
        self.git('merge', '--no-ff', 'feature', '-m', 'integration')
        self.E = self.git('rev-parse', 'HEAD')
        tool = Path(self.temp.name) / 'ripr'
        tool.write_text(ANALYZER)
        tool.chmod(0o755)
        self.calls = Path(self.temp.name) / 'calls.jsonl'
        self.env.update(RIPR_BIN=str(tool), ANALYZER_CALLS=str(self.calls))

    def write(self, path, text):
        p = self.repo / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def git(self, *args, input=None):
        p = subprocess.run(['git', '-C', str(self.repo), *args], input=input, env=self.env, text=True, capture_output=True, check=True)
        return p.stdout.strip()

    def commit(self, message):
        self.git('add', '.')
        self.git('commit', '-m', message)
        return self.git('rev-parse', 'HEAD')

    def run_xtask(self, *args, ok=True):
        result = subprocess.run([str(BINARY), '--worktree-root', str(self.repo), *args], cwd=self.repo, env=self.env, text=True, capture_output=True, timeout=30)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def command(self, name, *extra, head=None, pr=True):
        args = [name, '--base', self.A, '--head', head or self.E]
        if pr:
            args += ['--pr-head', self.H]
        return [*args, *extra]

    def packet(self, path):
        return json.loads((self.repo / path).read_text())

    def pr_packet(self):
        return self.packet('target/ripr/pr/repo-exposure.json')

    def generate(self):
        self.run_xtask(*self.command('ripr-pr'))
        self.run_xtask(*self.command('ripr-review-comments'))

    def gate(self, *extra, ok=True, pr=True):
        self.write('target/receipts/quality/ripr-plus.json', json.dumps(dict(schema_version=2, head=self.E, unresolved=0)))
        args = ['quality-gate', '--mode', 'enforce-new-ripr', '--exception-policy', 'exceptions.toml', '--ripr-base', self.A, '--ripr-head', self.E]
        if pr:
            args += ['--ripr-pr-head', self.H]
        return self.run_xtask(*args, *extra, ok=ok)

    def test_advanced_base_scope_and_all_provenance(self):
        self.generate()
        p = self.pr_packet()
        self.assertEqual(p['summary']['changed_files'], 1)
        subject = p['comparison_subject']
        self.assertEqual(subject, dict(schema_version='ripr-pr-comparison.v1', event_base_sha=self.A, pr_head_sha=self.H, comparison_base_sha=self.P, evaluated_sha=self.E, evaluated_tree_sha=self.git('rev-parse', self.E + '^{tree}')))
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()]
        check = next(args for args in calls if args[0] == 'check')
        self.assertEqual(check[check.index('--base') + 1], self.P)
        self.assertEqual(p['base_sha'], self.A)
        self.assertEqual(p['head_sha'], self.E)
        diff = (self.repo / 'target/ripr/pr/pr.diff').read_text()
        self.assertIn('scripts/change.py', diff)
        self.assertNotIn('src/lib.rs', diff)
        self.assertEqual(p['summary']['changed_files'], 1)
        self.run_xtask(*self.command('ripr-pr', '--check'))
        self.run_xtask(*self.command('ripr-review-comments', '--check'))
        self.gate()
        self.gate('--check')
        self.assertEqual(self.packet('target/receipts/quality/quality-gate.json')['comparison_subject'], subject)

    def test_guidance_native_arguments_use_comparison_parent(self):
        self.run_xtask(*self.command('ripr-review-comments'))
        calls = [json.loads(s) for s in self.calls.read_text().splitlines()]
        args = next((c for c in calls if c[0] == 'review-comments'))
        self.assertEqual(args[args.index('--base') + 1], self.P)
        self.assertEqual(args[args.index('--head') + 1], self.E)
        p = self.packet('target/ripr/review/comments.json')
        self.assertEqual(p['base_sha'], self.A)
        self.assertEqual(p['comparison_subject']['comparison_base_sha'], self.P)

    def test_merge_resolution_changes_are_included(self):
        self.write('src/resolved.rs', 'pub fn integrated() {}\n')
        self.git('add', 'src/resolved.rs')
        tree = self.git('write-tree')
        self.E = self.git('commit-tree', tree, '-p', self.P, '-p', self.H, input='integration resolution\n')
        self.git('reset', '--hard', self.E)
        self.run_xtask(*self.command('ripr-pr'))
        p = self.pr_packet()
        self.assertEqual(p['summary']['changed_files'], 2)
        receipt = self.packet('target/ripr/pr/committed-diff.json')
        self.assertIn('src/resolved.rs', receipt['changed_paths'])

    def test_neutralized_candidate_change_is_not_claimed_on_raw_head(self):
        tree = self.git('rev-parse', self.P + '^{tree}')
        self.E = self.git('commit-tree', tree, '-p', self.P, '-p', self.H, input='neutralized integration\n')
        self.git('reset', '--hard', self.E)
        self.run_xtask(*self.command('ripr-pr'))
        self.assertEqual(self.pr_packet()['summary']['changed_files'], 0)
        self.assertEqual(self.pr_packet()['comparison_subject']['pr_head_sha'], self.H)

    def test_stale_or_absent_scope_is_rejected_by_both_validators_and_gate(self):
        self.generate()
        paths = ['target/ripr/pr/repo-exposure.json', 'target/ripr/review/comments.json']
        for path in paths:
            original = self.packet(path)
            for field in [None, 'schema_version', 'event_base_sha', 'pr_head_sha', 'comparison_base_sha', 'evaluated_sha', 'evaluated_tree_sha']:
                with self.subTest(path=path, field=field):
                    mutant = copy.deepcopy(original)
                    if field is None:
                        mutant.pop('comparison_subject')
                    else:
                        mutant['comparison_subject'][field] = '0' * 40
                    self.write(path, json.dumps(mutant))
                    name = 'ripr-pr' if '/pr/' in path else 'ripr-review-comments'
                    self.run_xtask(*self.command(name, '--check'), ok=False)
                    self.gate(ok=False)
            self.write(path, json.dumps(original))
        self.gate()
        self.gate(pr=False, ok=False)
        self.gate('--ripr-baseline-commit', self.A, ok=False)

    def test_stale_clean_packet_cannot_skip_guidance_generation(self):
        self.generate()
        path = 'target/ripr/pr/repo-exposure.json'
        p = self.pr_packet()
        p.pop('comparison_subject')
        self.write(path, json.dumps(p))
        self.run_xtask(*self.command('ripr-review-comments'))
        calls = [json.loads(s) for s in self.calls.read_text().splitlines()]
        self.assertTrue(any((c[0] == 'review-comments' for c in calls)))

    def test_invalid_parent_raw_head_or_ancestry_refuses_before_analyzer(self):
        for head, raw, base in [(self.H, self.H, self.A), (self.E, self.A, self.A), (self.E, self.H, self.H), (self.E, 'f' * 40, self.A)]:
            with self.subTest(head=head, raw=raw, base=base):
                self.run_xtask('ripr-pr', '--base', base, '--head', head, '--pr-head', raw, ok=False)
                self.assertFalse(self.calls.exists())

    def test_no_pr_mode_keeps_original_merge_range(self):
        self.run_xtask(*self.command('ripr-pr', pr=False))
        p = self.pr_packet()
        self.assertNotIn('comparison_subject', p)
        self.assertEqual(p['summary']['changed_files'], 2)
        self.run_xtask(*self.command('ripr-pr', '--check', pr=False))

    def test_degraded_guidance_uses_only_current_scope_bound_raw_analysis(self):
        self.write('src/lib.rs', 'pub fn mainline() -> u8 { 3 }\npub fn introduced() -> u8 {\n    4\n}\n')
        self.git('add', 'src/lib.rs')
        tree = self.git('write-tree')
        self.E = self.git('commit-tree', tree, '-p', self.P, '-p', self.H, input='integration effect\n')
        self.git('reset', '--hard', self.E)
        self.env.update(ANALYZER_GAP='1', ANALYZER_BASE=self.P, ANALYZER_FAIL_GUIDANCE='1')
        self.run_xtask(*self.command('ripr-pr'))
        self.assertEqual(self.pr_packet()['summary']['severe_gaps'], 1)
        self.run_xtask(*self.command('ripr-review-comments'))
        valid = self.packet('target/ripr/review/comments.json')
        self.assertEqual(valid['mode'], 'pr_evidence_fallback')
        self.assertTrue(valid['summary_only'])
        self.gate(ok=False)
        rawpath = 'target/ripr/pr/raw-check.json'
        raw = self.packet(rawpath)
        raw['base'] = self.A
        self.write(rawpath, json.dumps(raw))
        self.run_xtask(*self.command('ripr-pr', '--check'), ok=False)
        self.run_xtask(*self.command('ripr-review-comments'))
        refused = self.packet('target/ripr/review/comments.json')
        self.assertEqual(refused['status'], 'error')
        self.assertEqual(refused['summary_only'], [])

    def test_coverage_only_mode_ignores_unrelated_ripr_subject(self):
        self.generate()
        self.write('target/receipts/quality/coverage-baseline.json', json.dumps(dict(schema_version=1, head=self.E, scope='workspace', coverage=dict(patch=100, project=100))))
        self.write('codecov.yml', 'coverage:\n  status:\n    patch:\n      default:\n        target: 95%\n        threshold: 0%\n')
        self.run_xtask('quality-gate', '--mode', 'enforce-patch-coverage', '--exception-policy', 'exceptions.toml')

    def test_refused_subject_invalidates_earlier_gate_success(self):
        self.generate()
        self.gate()
        self.assertTrue((self.repo / 'target/receipts/quality/quality-gate.json').exists())
        p = self.pr_packet()
        p['comparison_subject']['comparison_base_sha'] = self.A
        self.write('target/ripr/pr/repo-exposure.json', json.dumps(p))
        self.gate(ok=False)
        self.assertFalse((self.repo / 'target/receipts/quality/quality-gate.json').exists())
        self.assertFalse((self.repo / 'target/receipts/quality/quality-gate.md').exists())

    def test_wrong_evaluated_checkout_refuses_before_analysis(self):
        self.git('reset', '--hard', self.P)
        self.run_xtask(*self.command('ripr-pr'), ok=False)
        self.assertFalse(self.calls.exists())

    def test_producer_metadata_cannot_contradict_selected_comparison(self):
        self.env['ANALYZER_FORCE_BASE'] = self.A
        result = self.run_xtask(*self.command('ripr-pr'), ok=False)
        self.assertIn('raw producer comparison base', result.stderr)
        self.assertFalse((self.repo / 'target/ripr/pr/repo-exposure.json').exists())

    def test_dirty_tracked_or_untracked_source_cannot_claim_evaluated_tree(self):
        for path in ['src/lib.rs', 'src/untracked.rs']:
            with self.subTest(path=path):
                self.write(path, 'pub fn dirty() {}\n')
                result = self.run_xtask(*self.command('ripr-pr'), ok=False)
                self.assertIn('uncommitted source', result.stderr)
                self.assertFalse(self.calls.exists())
                if path == 'src/lib.rs':
                    self.git('restore', path)
                else:
                    (self.repo / path).unlink()

    def test_source_change_during_analyzer_execution_cannot_publish_pr_evidence(self):
        self.env['ANALYZER_DIRTY_SOURCE'] = '1'
        self.run_xtask(*self.command('ripr-pr'), ok=False)
        self.assertTrue(self.calls.exists())
        self.assertFalse((self.repo / 'target/ripr/pr/repo-exposure.json').exists())

    def test_equal_base_and_moving_main_preserve_frozen_scope(self):
        self.A = self.P
        self.generate()
        expected = self.pr_packet()['comparison_subject']
        self.git('update-ref', 'refs/remotes/origin/main', self.H)
        self.run_xtask(*self.command('ripr-pr', '--check'))
        self.assertEqual(self.pr_packet()['comparison_subject'], expected)
if __name__ == '__main__':
    unittest.main()
