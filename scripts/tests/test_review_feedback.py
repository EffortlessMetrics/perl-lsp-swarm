"""Offline acquisition regressions; no heuristic pretends to decide finding validity."""
import importlib.machinery
import importlib.util
import json
import os
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


class ThreadsShellSelection(unittest.TestCase):
    """Native-Windows Git Bash / POSIX helper-path selection (offline stubs)."""

    def _no_program_files(self):
        return patch.dict(os.environ, {'ProgramFiles': '',
                                       'ProgramFiles(x86)': '',
                                       'ProgramW6432': ''})

    def test_posix_uses_plain_bash_and_absolute_posix_threads_path(self):
        with patch.object(feedback, '_is_windows', return_value=False), \
                patch.object(feedback.shutil, 'which',
                             side_effect=AssertionError('no discovery on POSIX')):
            command = feedback._threads_command(7, 'o/r')
        self.assertEqual(command[0], 'bash')
        self.assertEqual(command[2:], ['7', 'o/r', '--json'])
        self.assertTrue(Path(command[1]).is_absolute())
        self.assertNotIn('\\', command[1])
        self.assertTrue(command[1].endswith('/threads'))

    def test_windows_selects_git_bash_not_wsl_shim(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'Git'
            (root / 'cmd').mkdir(parents=True)
            (root / 'bin').mkdir()
            git = root / 'cmd' / 'git.exe'
            bash = root / 'bin' / 'bash.exe'
            git.write_text('')
            bash.write_text('')
            shim = Path(tmp) / 'Windows' / 'System32' / 'bash.exe'
            shim.parent.mkdir(parents=True)
            shim.write_text('')
            which = lambda name: str(git) if name == 'git' else str(shim)
            with self._no_program_files(), \
                    patch.object(feedback, '_is_windows', return_value=True), \
                    patch.object(feedback.shutil, 'which', side_effect=which):
                command = feedback._threads_command(7, 'o/r')
        self.assertEqual(command[0], str(bash))
        self.assertNotIn('System32', command[0])
        self.assertNotIn('\\', command[1])
        self.assertTrue(Path(command[1]).is_absolute())
        self.assertTrue(command[1].endswith('/threads'))

    def test_windows_missing_git_bash_fails_honestly_without_fallback(self):
        with tempfile.TemporaryDirectory() as tmp:
            shim = Path(tmp) / 'Windows' / 'System32' / 'bash.exe'
            shim.parent.mkdir(parents=True)
            shim.write_text('')
            which = lambda name: str(shim) if name == 'bash' else None
            with self._no_program_files(), \
                    patch.object(feedback, '_is_windows', return_value=True), \
                    patch.object(feedback.shutil, 'which', side_effect=which):
                with self.assertRaisesRegex(OSError, 'Git Bash not found'):
                    feedback._bash_executable()

    def test_windows_command_flows_through_collect_in_same_call_order(self):
        sentinel = ['GIT_BASH', '/abs/threads', '7', 'o/r', '--json']
        replies = [{'head': {'sha': HEAD}},
                   {'threads': [], 'unresolved_count': 0},
                   [[]], [[]], [[]], {'head': {'sha': HEAD}}]
        with patch.object(feedback, '_threads_command',
                          return_value=sentinel) as threads_command, \
                patch.object(feedback, 'read_json', side_effect=replies) as read:
            result = feedback.collect(7, 'o/r')
        threads_command.assert_called_once_with(7, 'o/r')
        self.assertEqual(len(read.call_args_list), 6)
        self.assertEqual(read.call_args_list[1].args[0], sentinel)
        rest = [c.args[0] for c in read.call_args_list if '--slurp' in c.args[0]]
        self.assertEqual(len(rest), 3)
        for command in rest:
            self.assertIn('--paginate', command)
            self.assertTrue(command[-1].endswith('?per_page=100'))
        self.assertEqual(result['threads']['unresolved_count'], 0)

    def test_threads_failure_aborts_before_rest_pages(self):
        replies = [{'head': {'sha': HEAD}},
                   subprocess.CalledProcessError(127, ['bash'])]
        with patch.object(feedback, '_threads_command', return_value=['bash', 'threads']), \
                patch.object(feedback, 'read_json', side_effect=replies) as read:
            with self.assertRaises(subprocess.CalledProcessError):
                feedback.collect(7, 'o/r')
        self.assertEqual(len(read.call_args_list), 2)


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
