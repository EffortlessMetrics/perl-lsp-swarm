#!/usr/bin/env python3
"""Prove late-CI admission from actual workflow job expressions, without runners.

The evaluator intentionally accepts only the expression subset present at this
boundary and fails on new syntax. GitHub remains the execution authority. Draft
results are not proof; native readiness must produce the ordinary current checks.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
GUARD = "github.event_name != 'pull_request' || github.event.pull_request.draft != true"
TOKEN = re.compile(r"\s*(?:(?P<string>'(?:''|[^'])*')|(?P<op>\&\&|\|\||!=|==|[(),!])|(?P<name>[A-Za-z_][A-Za-z0-9_.*-]*))")


def expression(value):
    text = ' '.join(str(value).split())
    return text[3:-2].strip() if text.startswith('${{') and text.endswith('}}') else text


def evaluate(value, facts):
    """Evaluate the actual bounded boolean expression, rejecting unknown syntax."""
    text, output, offset = expression(value), [], 0
    functions = {
        'always': lambda: True,
        'contains': lambda values, item: item in values,
        'startsWith': lambda value, prefix: value.startswith(prefix),
        'endsWith': lambda value, suffix: value.endswith(suffix),
    }
    while offset < len(text):
        match = TOKEN.match(text, offset)
        if not match:
            raise AssertionError(f'unsupported Actions expression: {text[offset:]}')
        offset = match.end()
        if match.lastgroup == 'string':
            output.append(repr(match.group('string')[1:-1].replace("''", "'")))
        elif match.lastgroup == 'op':
            output.append({'&&': ' and ', '||': ' or ', '!': ' not '}.get(match.group('op'), match.group('op')))
        else:
            name = match.group('name')
            if name in functions:
                output.append(name)
            elif name in ('true', 'false', 'True', 'False'):
                output.append(str(name.lower() == 'true'))
            else:
                output.append(repr(facts.get(name, '')))
    return bool(eval(''.join(output), {'__builtins__': {}}, functions))


def workflows():
    for path in sorted((ROOT / '.github/workflows').glob('*.y*ml')):
        data = yaml.safe_load(path.read_text())
        events = data.get('on', data.get(True, {}))
        if isinstance(events, dict) and 'pull_request' in events:
            yield path.name, data, events


def admitted_remainder(condition):
    expr = expression(condition)
    if expr.lower() == 'false':
        return 'false'
    if expr == GUARD:
        return 'true'
    prefix = f'({GUARD}) && ('
    if not expr.startswith(prefix) or not expr.endswith(')'):
        raise AssertionError(f'missing pre-allocation late-CI guard: {expr}')
    return expr[len(prefix):-1]


def facts_for(event, draft, workflow_data, route='github', result='success'):
    facts = {
        'github.event_name': event,
        'github.event.pull_request.draft': draft,
        'github.event.action': 'ready_for_review',
        'github.repository': 'EffortlessMetrics/perl-lsp-swarm',
        'github.event.pull_request.head.repo.full_name': 'EffortlessMetrics/perl-lsp-swarm',
        'github.event.pull_request.user.type': 'User',
        'github.event.pull_request.user.login': 'maintainer',
        'github.ref': 'refs/heads/main',
        'github.ref_name': 'main',
        'github.head_ref': 'repair/11983-current-main',
        'github.event.repository.default_branch': 'main',
        'github.event.inputs.mode': 'bounded',
        'github.event.label.name': 'ci:public-api',
        'github.event.pull_request.labels.*.name': [
            'ci:tokmd', 'ci:perl-matrix', 'ci:mutation', 'ci:bench', 'ci:real-repo-latency',
            'ci:corpus-differential', 'ci:memory', 'ci:strict', 'ci:semver',
            'ci:public-api', 'ci:metrics-ratchet', 'ci:kwalitee',
        ],
    }
    for _, data, _ in workflow_data:
        for job in data['jobs']:
            facts[f'needs.{job}.result'] = result
            for output in ('run_ci', 'is_latest', 'windows_runner', 'fallback_allowed', 'preflight_ok', 'api_scope', 'complete', 'changed'):
                facts[f'needs.{job}.outputs.{output}'] = 'true'
            facts[f'needs.{job}.outputs.target'] = route
    for flag in ('run_mutation', 'run_benchmarks', 'run_real_repo_latency', 'run_corpus_differential', 'run_memory', 'run_coverage', 'run_tautology', 'run_semver', 'run_public_api', 'run_scorecard', 'run_clippy_strict', 'run_perl_kwalitee', 'run_fuzz', 'signatures_only'):
        facts[f'inputs.{flag}'] = True
    return facts


class LateCiContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflows = list(workflows())
        cls.facts = {(event, draft, route, result): facts_for(event, draft, cls.workflows, route, result)
                     for event in ('pull_request', 'push', 'merge_group', 'workflow_dispatch', 'schedule')
                     for draft in (False, True)
                     for route in ('github', 'selfhosted', 'cx53', 'cx43')
                     for result in ('success', 'failure', 'cancelled', 'skipped')}

    def test_every_ordinary_pr_workflow_wakes_on_ready(self):
        self.assertGreater(len(self.workflows), 60)
        for name, _, events in self.workflows:
            with self.subTest(workflow=name):
                self.assertIn('ready_for_review', (events['pull_request'] or {}).get('types', []))

    def test_all_draft_jobs_are_quiet_before_allocation(self):
        for name, data, _ in self.workflows:
            for job, value in data['jobs'].items():
                with self.subTest(workflow=name, job=job):
                    admitted_remainder(value.get('if', 'true'))
                    for (event, draft, _, _), facts in self.facts.items():
                        if event == 'pull_request' and draft:
                            self.assertFalse(evaluate(value.get('if', 'true'), facts))

    def test_guard_is_transparent_for_ready_and_non_pr_subjects(self):
        for name, data, _ in self.workflows:
            for job, value in data['jobs'].items():
                with self.subTest(workflow=name, job=job):
                    condition = value.get('if', 'true')
                    original = admitted_remainder(condition)
                    for (event, draft, _, _), facts in self.facts.items():
                        if event != 'pull_request' or not draft:
                            self.assertEqual(evaluate(condition, facts), evaluate(original, facts))

    def test_required_result_jobs_still_run_after_failure_when_ready(self):
        for name, job in [('ci.yml', 'merge-gate'), ('em-ci-routed-rust.yml', 'rust-small-result'), ('ripr.yml', 'ripr')]:
            data = next(data for filename, data, _ in self.workflows if filename == name)
            condition = data['jobs'][job]['if']
            self.assertIn('always()', condition)
            for result in ('success', 'failure', 'cancelled', 'skipped'):
                self.assertTrue(evaluate(condition, self.facts[('pull_request', False, 'github', result)]))

    def test_admission_rejects_missing_or_disjoined_guard(self):
        for mutant in ('true', 'always()', f'({GUARD}) || (always())', f"(github.event_name != 'pull_request') && (always())"):
            with self.subTest(mutant=mutant), self.assertRaises(AssertionError):
                admitted_remainder(mutant)

    def test_evaluator_rejects_unknown_syntax(self):
        with self.assertRaises(AssertionError):
            evaluate('github.event_name + true', {})


if __name__ == '__main__':
    unittest.main()
