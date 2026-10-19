"""Offline acquisition regressions; no heuristic pretends to decide finding validity."""
import importlib.machinery
import importlib.util
import io
import json
import os
from contextlib import redirect_stdout
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/reviews/feedback'
loader = importlib.machinery.SourceFileLoader('feedback', str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
feedback = importlib.util.module_from_spec(spec)
loader.exec_module(feedback)
HEAD = 'a' * 40


def item(n, body, **fields):
    return dict(id=n, body=body, html_url=f'https://example.invalid/comments/{n}', **fields)


class FeedbackAcquisition(unittest.TestCase):
    def collect(self, *, comments=None, reviews=None, inline=None, second_head=HEAD):
        replies = [
            {'head': {'sha': HEAD}},
            {'threads': [], 'unresolved_count': 0},
            inline if inline is not None else [[]],
            comments if comments is not None else [[]],
            reviews if reviews is not None else [[]],
            {'head': {'sha': second_head}},
        ]
        with patch.object(feedback, 'read_json', side_effect=replies) as read:
            result = feedback.collect(7, 'o/r')
        return result, read.call_args_list

    def test_zero_threads_does_not_hide_ordinary_comment_finding(self):
        finding = item(4, 'REPAIR_REQUIRED: action 6 still authorizes an empty push')
        result, _ = self.collect(comments=[[finding]])
        self.assertEqual(result['threads']['unresolved_count'], 0)
        self.assertEqual(result['issue_comments'], [finding])
        self.assertEqual(result['next_step'], 'address-review-comments')
        self.assertTrue(result['content_judgment_required'])
        self.assertEqual(result['review_currentness'], 'NOT_PROVEN')

    def test_commented_metadata_preserves_adverse_body_and_later_disposition(self):
        finding = item(4, 'CHANGES_REQUIRED: current branch violates the contract', state='COMMENTED')
        disposition = item(5, 'Disposition: fixed\nEvidence: repair commit and focused red/green test')
        result, _ = self.collect(reviews=[[finding]], comments=[[disposition]])
        self.assertEqual(result['reviews'], [finding])
        self.assertEqual(result['issue_comments'], [disposition])
        # A later comment is data for judgment, never automatic green.
        self.assertEqual(result['review_currentness'], 'NOT_PROVEN')

    def test_every_rest_surface_requests_all_pages_and_retains_last_page(self):
        first = [item(n, 'x' * 5000) for n in range(1, 101)]
        last = item(101, 'Only page two reveals the remaining blocker', in_reply_to_id=1)
        result, calls = self.collect(inline=[first, [last]],
                                     comments=[first, [last]], reviews=[first, [last]])
        for key in ('inline_comments', 'issue_comments', 'reviews'):
            self.assertEqual(len(result[key]), 101)
            self.assertEqual(result[key][-1], last)
        rest = [call.args[0] for call in calls if '--slurp' in call.args[0]]
        self.assertEqual(len(rest), 3)
        for command in rest:
            self.assertIn('--paginate', command)
            self.assertTrue(command[-1].endswith('?per_page=100'))

    def test_head_change_during_read_refuses_current_binding(self):
        with self.assertRaisesRegex(ValueError, 'head moved'):
            self.collect(second_head='b' * 40)

    def test_truncated_or_malformed_rest_envelope_is_not_empty_feedback(self):
        for raw in ([], {}, [{'id': 1}], [[{'id': 1}]], [[item(1, 'x')], [item(1, 'x')]]):
            with self.subTest(raw=str(raw)[:50]), patch.object(feedback, 'read_json', return_value=raw):
                with self.assertRaises(ValueError):
                    feedback.pages('o/r', 'issues/7/comments')

    def test_cli_failures_return_machine_next_step_not_clean_inventory(self):
        with tempfile.TemporaryDirectory() as tmp:
            stub = Path(tmp) / 'gh'
            stub.write_text('#!/bin/sh\nexit 1\n')
            stub.chmod(0o755)
            run = subprocess.run(['python3', str(SCRIPT), '7', 'o/r'],
                                 env={**os.environ, 'PATH': tmp + os.pathsep + os.environ['PATH']},
                                 text=True, capture_output=True)
        self.assertEqual(run.returncode, 2)
        result = json.loads(run.stdout)
        self.assertEqual(result['next_step'], 'repair-feedback-acquisition')
        self.assertEqual(result['review_currentness'], 'NOT_PROVEN')
        self.assertNotIn('issue_comments', result)

    def test_cli_joins_real_thread_paginator_and_full_rest_pages(self):
        # Exercise both actual command boundaries; the other tests isolate
        # malformed REST envelopes and subject movement without network access.
        def thread(n):
            return dict(id=f'PRRT_{n}', isResolved=True, isOutdated=False,
                        comments={'nodes': [{'body': 'opener', 'url': f'https://example.invalid/{n}'}]})

        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp) / 'data.json'
            data.write_text(json.dumps({
                'one': {'data': {'repository': {'pullRequest': {'reviewThreads': {
                    'nodes': [thread(1)], 'pageInfo': {'hasNextPage': True, 'endCursor': 'page2'}
                }}}}},
                'two': {'data': {'repository': {'pullRequest': {'reviewThreads': {
                    'nodes': [thread(2)], 'pageInfo': {'hasNextPage': False, 'endCursor': None}
                }}}}},
                'inline': [[item(1, 'full opener')], [item(2, 'full reply', in_reply_to_id=1)]],
                'ordinary': [[item(3, 'prior context')], [item(4, 'REPAIR_REQUIRED: still broken')]],
                'reviews': [[item(5, 'CHANGES_REQUIRED: repair needed', state='COMMENTED')]],
            }))
            stub = Path(tmp) / 'gh'
            stub.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
data=json.loads(Path(os.environ['FEEDBACK_DATA']).read_text())
args=sys.argv[1:]
if args[:2]==['api','graphql']:
    result=data['two' if 'after: "page2"' in ' '.join(args) else 'one']
elif args[-1]=='repos/o/r/pulls/7':
    result={'head':{'sha':'a'*40}}
else:
    assert '--paginate' in args and '--slurp' in args
    endpoint=args[-1]
    result=data['ordinary' if '/issues/' in endpoint else 'reviews' if '/reviews?' in endpoint else 'inline']
print(json.dumps(result))
''')
            stub.chmod(0o755)
            run = subprocess.run(['python3', str(SCRIPT), '7', 'o/r'],
                                 env={**os.environ, 'FEEDBACK_DATA': str(data),
                                      'PATH': tmp + os.pathsep + os.environ['PATH']},
                                 text=True, capture_output=True, timeout=10)
        self.assertEqual(run.returncode, 0, run.stderr)
        result = json.loads(run.stdout)
        self.assertEqual(result['threads']['thread_count'], 2)
        self.assertEqual(result['threads']['unresolved_count'], 0)
        self.assertEqual(result['inline_comments'][-1]['body'], 'full reply')
        self.assertIn('REPAIR_REQUIRED', result['issue_comments'][-1]['body'])
        self.assertEqual(result['reviews'][0]['state'], 'COMMENTED')
        self.assertTrue(result['content_judgment_required'])
        self.assertEqual(result['review_currentness'], 'NOT_PROVEN')


class FeedbackCommandDiagnostics(unittest.TestCase):
    def run_cli(self, stderr, *, stdout=b'PRIVATE_STDOUT_SHOULD_NOT_ESCAPE',
                exit_code=7, expected_reason='CalledProcessError'):
        stderr_bytes = stderr.encode('utf-8') if isinstance(stderr, str) else stderr
        stdout_bytes = stdout.encode('utf-8') if isinstance(stdout, str) else stdout
        with tempfile.TemporaryDirectory() as tmp:
            calls = Path(tmp) / 'calls.jsonl'
            stub = Path(tmp) / 'gh'
            stub.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with Path(os.environ['DIAGNOSTIC_CALLS']).open('a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\\n')
sys.stdout.buffer.write(bytes.fromhex(os.environ['DIAGNOSTIC_STDOUT_HEX']))
sys.stderr.buffer.write(bytes.fromhex(os.environ['DIAGNOSTIC_STDERR_HEX']))
sys.exit(int(os.environ['DIAGNOSTIC_EXIT']))
''')
            stub.chmod(0o755)
            run = subprocess.run(
                ['python3', str(SCRIPT), '7', 'o/r'],
                env={**os.environ, 'PATH': tmp + os.pathsep + os.environ['PATH'],
                     'DIAGNOSTIC_CALLS': str(calls), 'DIAGNOSTIC_STDERR_HEX': stderr_bytes.hex(),
                     'DIAGNOSTIC_STDOUT_HEX': stdout_bytes.hex(), 'DIAGNOSTIC_EXIT': str(exit_code)},
                text=True, capture_output=True, timeout=10,
            )
            invocations = [json.loads(line) for line in calls.read_text().splitlines()]
        self.assertEqual(invocations, [['api', 'repos/o/r/pulls/7']])
        self.assertEqual(run.returncode, 2)
        self.assertEqual(run.stderr, '')
        self.assertNotIn('PRIVATE_STDOUT_SHOULD_NOT_ESCAPE', run.stdout)
        result = json.loads(run.stdout)
        self.assertEqual(result['reason'], expected_reason)
        self.assertEqual(result['next_step'], 'repair-feedback-acquisition')
        self.assertEqual(result['review_currentness'], 'NOT_PROVEN')
        self.assertNotIn('issue_comments', result)
        if expected_reason == 'CalledProcessError':
            self.assertIn('command_failure', result, 'child failure context was discarded')
            self.assertEqual(result['command_failure']['operation'], 'head-before')
            self.assertEqual(result['command_failure']['command'], 'gh api')
            self.assertEqual(result['command_failure']['exit_code'], exit_code)
        return result, run.stdout

    def test_cli_reports_denial_and_transport_without_retrying(self):
        cases = (
            ('Get "https://api.github.com/private?token=FAKE_SECRET": Forbidden', 'access_denied'),
            ('gh: Resource not accessible by integration (HTTP 403)', 'access_denied'),
            ('gh: authentication failed (HTTP 401)', 'access_denied'),
            ('net/http: TLS handshake timeout; FAKE_SECRET', 'transport_error'),
            ('timeout while reporting Forbidden; FAKE_SECRET', 'access_denied'),
        )
        for stderr, expected in cases:
            with self.subTest(stderr=stderr):
                result, output = self.run_cli(stderr)
                self.assertEqual(result['command_failure']['stderr']['kind'], expected)
                self.assertFalse(result['command_failure']['stderr']['truncated'])
                self.assertNotIn('FAKE_SECRET', output)
                self.assertNotIn('https://api.github.com/private', output)

    def test_unknown_empty_and_oversized_stderr_remain_sanitized(self):
        cases = (
            ('Authorization: Bearer FAKE_SECRET\n\x1b[31mPRIVATE_MESSAGE', 'unrecognized', False),
            ('PRIVATE_MESSAGE' * 1000 + ' Forbidden FAKE_SECRET', 'unrecognized', True),
            ('', 'empty', False),
        )
        for stderr, kind, truncated in cases:
            with self.subTest(kind=kind, truncated=truncated):
                result, output = self.run_cli(stderr)
                self.assertEqual(result['command_failure']['stderr']['kind'], kind)
                self.assertEqual(result['command_failure']['stderr']['truncated'], truncated)
                self.assertLess(len(output), 2048)
                for private in ('FAKE_SECRET', 'PRIVATE_MESSAGE', 'Authorization:', '\x1b'):
                    self.assertNotIn(private, output)

    def test_non_utf8_child_failure_keeps_context_and_sanitizes_both_streams(self):
        for stdout, stderr, kind in (
            (b'PRIVATE_STDOUT', b'HTTP 403 Forbidden \xff FAKE_SECRET', 'access_denied'),
            (b'PRIVATE_STDOUT\xff', b'TLS handshake timeout FAKE_SECRET', 'transport_error'),
            (b'PRIVATE_STDOUT\xff', b'\xff FAKE_SECRET', 'unrecognized'),
        ):
            with self.subTest(kind=kind):
                result, output = self.run_cli(stderr, stdout=stdout)
                self.assertEqual(result['command_failure']['stderr']['kind'], kind)
                self.assertNotIn('FAKE_SECRET', output)
                self.assertNotIn('PRIVATE_STDOUT', output)

    def test_successful_child_with_invalid_utf8_json_remains_refused(self):
        result, output = self.run_cli(
            b'', stdout=b'{"head":{"sha":"' + b'a' * 39 + b'\xff"}}',
            exit_code=0, expected_reason='UnicodeDecodeError',
        )
        self.assertNotIn('command_failure', result)
        self.assertNotIn('observed_head', result)

    def test_failure_context_survives_each_acquisition_boundary(self):
        replies = (
            {'head': {'sha': HEAD}},
            {'threads': [], 'unresolved_count': 0},
            [[]], [[]], [[]], {'head': {'sha': HEAD}},
        )
        operations = ('head-before', 'review-threads', 'inline-comments',
                      'issue-comments', 'reviews', 'head-after')
        for index, operation in enumerate(operations):
            with self.subTest(operation=operation):
                completed = [subprocess.CompletedProcess([], 0, json.dumps(row).encode('utf-8'), b'')
                             for row in replies[:index]]
                error = subprocess.CalledProcessError(
                    19, ['UNSAFE_COMMAND', 'FAKE_SECRET'],
                    output='PRIVATE_STDOUT', stderr='Forbidden FAKE_SECRET',
                )
                output = io.StringIO()
                with patch.object(feedback.subprocess, 'run', side_effect=[*completed, error]) as run, \
                        patch('sys.argv', ['feedback', '7', 'o/r']), redirect_stdout(output):
                    self.assertEqual(feedback.main(), 2)
                result = json.loads(output.getvalue())
                self.assertIn('command_failure', result, 'child failure context was discarded')
                failure = result['command_failure']
                self.assertEqual(failure['operation'], operation)
                self.assertEqual(failure['command'], 'bash scripts/reviews/threads'
                                 if index == 1 else 'gh api')
                self.assertEqual(failure['exit_code'], 19)
                self.assertEqual(failure['stderr']['kind'], 'access_denied')
                self.assertEqual(run.call_count, index + 1, 'acquisition retried or continued')
                for private in ('UNSAFE_COMMAND', 'FAKE_SECRET', 'PRIVATE_STDOUT'):
                    self.assertNotIn(private, output.getvalue())

    def test_success_preserves_json_and_single_subprocess_execution(self):
        command = ['gh', 'api', 'repos/o/r/pulls/7']
        value = {'head': {'sha': HEAD}, 'message': 'café'}
        completed = subprocess.CompletedProcess(
            command, 0, json.dumps(value, ensure_ascii=False).encode('utf-8'), b'PRIVATE_STDERR',
        )
        with patch.object(feedback.subprocess, 'run', return_value=completed) as run:
            self.assertEqual(feedback.read_json(command), value)
        run.assert_called_once_with(command, check=True, capture_output=True)

    def test_other_failures_keep_the_existing_safe_envelope(self):
        output = io.StringIO()
        with patch.object(feedback, 'collect', side_effect=ValueError('FAKE_SECRET')), \
                patch('sys.argv', ['feedback', '7', 'o/r']), redirect_stdout(output):
            self.assertEqual(feedback.main(), 2)
        result = json.loads(output.getvalue())
        self.assertEqual(result['reason'], 'ValueError')
        self.assertNotIn('command_failure', result)
        self.assertNotIn('FAKE_SECRET', output.getvalue())


class FinishingSkillScenarios(unittest.TestCase):
    """Prompt-contract regressions complement acquisition/currentness executable tests.

    These verify the active provider routes carry the obligation. They do not claim
    to measure agent compliance, receiver acceptance, hosted CI, or protected merge.
    """
    def texts(self, skill):
        for provider in ('.agents', '.claude'):
            path = ROOT / provider / 'skills' / skill / 'SKILL.md'
            yield provider, ' '.join(path.read_text().split())

    def test_unowned_inflight_handoff_cannot_transfer_continuation(self):
        for provider, text in self.texts('verify-live-ci'):
            with self.subTest(provider=provider):
                for token in ('continuation_owner', 'next_action', 'wake_event',
                              'require receiver acceptance', 'Missing ownership is `NOT_PROVEN`',
                              'terminal run/attempt inspection'):
                    self.assertIn(token, text)

    def test_zero_threads_commented_concern_still_enters_repair(self):
        for provider, text in self.texts('address-review-comments'):
            with self.subTest(provider=provider):
                for token in ('scripts/reviews/feedback', '`COMMENTED` is submission metadata',
                              '`REPAIR_REQUIRED`', 'source URL', '--body-file',
                              'Re-read feedback after posting', 'Zero unresolved threads alone never',
                              'Open substantive ordinary-comment findings are `MUTABLE_FINDINGS_OPEN`'):
                    self.assertIn(token, text)

    def test_material_head_change_cannot_reuse_stale_review_binding(self):
        for provider, text in self.texts('finish-pr'):
            with self.subTest(provider=provider):
                self.assertIn('--expected-head <observed-head>', text)
                self.assertIn('A stale or missing binding is `NOT_PROVEN`', text)
                self.assertIn('Preserve unaffected proof with its original subject', text)

    def test_badge_is_not_execution_and_final_judgment_is_separate(self):
        for provider, text in self.texts('verify-live-ci'):
            with self.subTest(provider=provider):
                self.assertIn('branch rulesets AND classic branch protection', text)
                self.assertIn('`draft-no-proof`', text)
                self.assertIn('mandatory selected proof still needs actual execution', text)
        for provider, text in self.texts('merge-reconcile'):
            with self.subTest(provider=provider):
                self.assertIn('accepted product vision', text)
                self.assertIn('Record that separate final judgment', text)
                self.assertIn('read the durable PR/commit state', text)


if __name__ == '__main__':
    unittest.main()
