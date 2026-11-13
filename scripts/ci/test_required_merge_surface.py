"""One-source and fail-closed checks for #19262's complete merge-surface proof."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
BASELINE = '032468df9b9912ab847e5f5243b7f4897b8465b9'
SCRIPT = ROOT / 'scripts/ci/required-merge-surface.sh'


def original_steps():
    yml = subprocess.check_output([
        'git', '-C', str(ROOT), 'show', f'{BASELINE}:.github/workflows/ci.yml',
    ]).decode('utf-8')
    steps = yaml.safe_load(yml)['jobs']['check-all-targets']['steps']
    assert len(steps) == 43
    selected = [s for s in steps if s.get('name', '').endswith('(required merge surface)')]
    assert len(selected) == 38
    return selected


def producer_commands(script=SCRIPT):
    text = script.read_text(encoding='utf-8')
    starts = list(re.finditer(r'^# required merge surface command \d+: .*$', text, re.M))
    commands = []
    for index, marker in enumerate(starts):
        start = marker.end() + 1
        end = starts[index + 1].start() if index + 1 < len(starts) else len(text)
        segment = text[start:end]
        # Ignore only the producer's own comment and progress bookkeeping.
        lines = [line for line in segment.splitlines()
                 if not line.startswith('_step_index=')]
        commands.append('\n'.join(lines).strip())
    return commands


def run_with_tools(script, tools, extra_env=None):
    output_dir = tempfile.TemporaryDirectory()
    tools_dir = Path(output_dir.name)
    env = os.environ.copy()
    for tool in ['bash', 'dirname', 'mkdir', 'rm', 'awk', 'grep', *tools]:
        shutil.copy2(shutil.which(tool), tools_dir / tool)
    if 'cargo' in tools:
        # This fixture checks the missing-tool refusal after the guard, not a
        # real build. Keep the guard's version probe separate from executions.
        fake_cargo = tools_dir / 'cargo'
        fake_cargo.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "$*" == "--version" ]]; then\n'
            '    printf "cargo 1.100.0\\n"\n'
            '    exit 0\n'
            'fi\n'
            'exit 97\n', encoding='utf-8')
        fake_cargo.chmod(0o755)
    env['PATH'] = str(tools_dir)
    env['RUNNER_TEMP'] = str(tools_dir)
    if extra_env:
        env.update(extra_env)
    return output_dir, subprocess.run([shutil.which('bash'), str(script)], cwd=ROOT, env=env,
                                      capture_output=True, text=True, timeout=120)


class RequiredMergeSurfaceTests(unittest.TestCase):
    def test_original_commands_are_an_unchanged_ordered_prefix(self):
        expected = [step['run'].strip() for step in original_steps()]
        actual = producer_commands()
        self.assertEqual(actual[:len(expected)], expected)
        self.assertGreaterEqual(len(actual), 38)

    def test_appending_a_contract_command_needs_only_the_producer(self):
        keeper = tempfile.TemporaryDirectory()
        self.addCleanup(keeper.cleanup)
        altered = Path(keeper.name) / 'required-merge-surface.sh'
        altered.write_text(
            SCRIPT.read_text(encoding='utf-8')
            + '\n# required merge surface command 39: Future contract (required merge surface)\n'
            + '_step_index=$((_step_index + 1))\n'
            + 'cargo test -p xtask --test future_contract --locked -- --nocapture\n',
            encoding='utf-8',
        )
        expected = [step['run'].strip() for step in original_steps()]
        actual = producer_commands(altered)
        self.assertEqual(actual[:len(expected)], expected)
        self.assertEqual(len(actual), len(expected) + 1)

    def test_ci_retains_setup_compile_and_exactly_one_shared_producer(self):
        data = yaml.safe_load((ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8'))
        steps = data['jobs']['check-all-targets']['steps']
        self.assertEqual(len(steps), 6)
        self.assertEqual(steps[4]['name'], 'Compile all targets (catches integration-test + bench bit-rot)')
        self.assertEqual(steps[4]['run'], 'just check-all-targets')
        self.assertEqual(steps[5]['run'], 'bash scripts/ci/required-merge-surface.sh')
        self.assertEqual(sum(s.get('name', '').endswith('(required merge surface)') for s in steps), 0)

    def test_admission_route_is_separate_from_pr_fast_and_non_skippable(self):
        justfile = (ROOT / 'justfile').read_text(encoding='utf-8')
        self.assertRegex(justfile, r'(?m)^prove-required-merge-surface:$')
        self.assertRegex(justfile, r'(?m)^ci-contracts: prove-required-merge-surface$')
        self.assertIn('bash scripts/ci/required-merge-surface.sh', justfile)
        policy = (ROOT / '.ci/gate-policy.yaml').read_text(encoding='utf-8')
        self.assertNotIn('required-merge-surface.sh', policy)
        self.assertIn('target_duration_seconds: 180', policy)

    def seed_stale_pass_receipt(self):
        receipt = ROOT / 'target/receipts/required-merge-surface.json'
        receipt.parent.mkdir(parents=True, exist_ok=True)
        receipt.write_text('{"result":"pass","head":"stale"}\n', encoding='utf-8')
        return receipt

    def test_filtered_step_is_not_green(self):
        receipt = self.seed_stale_pass_receipt()
        keeper, result = run_with_tools(SCRIPT, [], {'REQUIRED_MERGE_SURFACE_ONLY': 'one'})
        self.addCleanup(keeper.cleanup)
        self.assertEqual(result.returncode, 81)
        self.assertIn('NOT_PROVEN', result.stderr)
        self.assertFalse(receipt.exists(), 'an early refusal must invalidate stale green proof')

    def test_missing_toolchain_is_not_green(self):
        receipt = self.seed_stale_pass_receipt()
        keeper, result = run_with_tools(SCRIPT, ['cargo', 'git', 'python3', 'jq'])
        self.addCleanup(keeper.cleanup)
        self.assertEqual(result.returncode, 82)
        self.assertIn('required tool just is unavailable', result.stderr)
        self.assertIn('NOT_PROVEN', result.stderr)
        self.assertFalse(receipt.exists(), 'an early refusal must invalidate stale green proof')

    def test_filtered_command_body_fails_equivalence_check(self):
        keeper = tempfile.TemporaryDirectory()
        self.addCleanup(keeper.cleanup)
        altered = Path(keeper.name) / 'required-merge-surface.sh'
        text = SCRIPT.read_text(encoding='utf-8').replace(
            '  --test-threads=1 --color never', '  --test-threads=2 --color never', 1,
        )
        self.assertNotEqual(text, SCRIPT.read_text(encoding='utf-8'))
        altered.write_text(text, encoding='utf-8')
        expected = [step['run'].strip() for step in original_steps()]
        self.assertNotEqual(producer_commands(altered)[:len(expected)], expected)

    def test_missing_runner_temp_is_intentional_not_proven(self):
        keeper, result = run_with_tools(SCRIPT, [], {'RUNNER_TEMP': ''})
        self.addCleanup(keeper.cleanup)
        self.assertEqual(result.returncode, 80)
        self.assertIn('RUNNER_TEMP must name an existing artifact directory', result.stderr)
        self.assertIn('NOT_PROVEN', result.stderr)

    def test_untracked_and_ignored_foreign_inputs_are_not_green(self):
        for relative in [
            'proof-foreign-input-do-not-commit.md',
            'xtask/src/tasks/standalone_vectors/temp_oracle.rs',
        ]:
            with self.subTest(relative=relative):
                foreign = ROOT / relative
                self.assertFalse(foreign.exists(), f'refusing to overwrite {relative}')
                foreign.write_text('# Synthetic foreign input; proof must refuse it.\n', encoding='utf-8')
                self.addCleanup(foreign.unlink)
                tools = ['cargo', 'just', 'git', 'python3', 'jq']
                keeper, result = run_with_tools(SCRIPT, tools)
                self.addCleanup(keeper.cleanup)
                self.assertEqual(result.returncode, 86, result.stderr)
                self.assertIn('NOT_PROVEN', result.stderr)

    def test_deliberate_command_failure_is_red_in_script_and_local_admission(self):
        keeper = tempfile.TemporaryDirectory()
        self.addCleanup(keeper.cleanup)
        bin_dir = Path(keeper.name) / 'bin'
        bin_dir.mkdir()
        spy = Path(keeper.name) / 'attempt.log'
        fake_cargo = bin_dir / 'cargo'
        fake_cargo.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "$*" == "--version" ]]; then\n'
            '    printf "cargo 1.100.0\\n"\n'
            '    exit 0\n'
            'fi\n'
            'printf "cargo %s\\n" "$*" >> "$MERGE_SURFACE_SPY"\n'
            'exit 97\n', encoding='utf-8')
        fake_cargo.chmod(0o755)
        for tool in ['bash', 'git', 'just', 'python3', 'jq']:
            shutil.copy2(shutil.which(tool), bin_dir / tool)
        env = os.environ.copy()
        env['PATH'] = str(bin_dir) + os.pathsep + env['PATH']
        env['RUNNER_TEMP'] = str(Path(keeper.name) / 'artifacts')
        Path(env['RUNNER_TEMP']).mkdir()
        env['MERGE_SURFACE_SPY'] = str(spy)
        for command in [['bash', str(SCRIPT)], [shutil.which('just'), 'prove-required-merge-surface']]:
            with self.subTest(command=command):
                spy.write_text('', encoding='utf-8')
                result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=120)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(spy.read_text(encoding='utf-8').count('gates_1469_pr_required_static_checks'), 1)
        receipt = json.loads((ROOT / 'target/receipts/required-merge-surface.json').read_text(encoding='utf-8'))
        self.assertEqual(receipt['result'], 'not_proven')
        self.assertNotEqual(receipt['exit_code'], 0)
        self.assertEqual(receipt['commands_attempted'], 1)

    def test_producer_is_tracked_and_receipt_ignored(self):
        subprocess.run(['git', '-C', str(ROOT), 'ls-files', '--error-unmatch', '--',
                        'scripts/ci/required-merge-surface.sh'], check=True, capture_output=True)
        subprocess.run(['git', '-C', str(ROOT), 'check-ignore', '--quiet', '--',
                        'target/receipts/required-merge-surface.json'], check=True)


if __name__ == '__main__':
    unittest.main()
