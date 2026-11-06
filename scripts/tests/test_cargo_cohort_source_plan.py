"""Fresh source-validation controls; these tests do not launch a cohort."""
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_cohort as subject


@unittest.skipUnless(sys.platform == 'linux', 'native Linux source path contract')
class SourceTraversalPlan(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='cohort-source-plan-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.entries = []

    def file(self, relative, data=b'original source\n'):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        self.entries.append({'path': str(path), 'sha256': hashlib.sha256(data).hexdigest()})
        return path

    def prepare(self):
        return subject._prepare_source_declaration(self.root, self.entries)

    def run_plan(self, plan):
        subject._validate_source_declaration(self.root, plan)

    def test_preparation_does_not_observe_the_filesystem(self):
        self.file('a/b/source.rs')
        with patch.object(os, 'open', side_effect=AssertionError('opened during preparation')), \
             patch.object(os, 'stat', side_effect=AssertionError('stat during preparation')), \
             patch.object(os, 'fstat', side_effect=AssertionError('fstat during preparation')):
            plan = self.prepare()
        self.run_plan(plan)

    def test_repeated_validation_reuses_only_lexical_walk(self):
        self.file('a/b/one.rs')
        self.file('a/c/two.rs')
        self.file('d/three.rs')
        plan = self.prepare()
        with patch.object(Path, '__truediv__', side_effect=AssertionError('rebuilt lexical path')), \
             patch.object(Path, 'joinpath', side_effect=AssertionError('rebuilt lexical path')):
            self.run_plan(plan)
            self.run_plan(plan)

    def test_cached_plan_rehashes_same_size_restored_mtime_edit(self):
        path = self.file('a/source.rs', b'aaaa')
        plan = self.prepare()
        self.run_plan(plan)
        before = path.stat()
        path.write_bytes(b'bbbb')
        os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
        with self.assertRaises(subject.Denied):
            self.run_plan(plan)

    def test_same_content_replacement_between_calls_is_still_allowed(self):
        path = self.file('a/source.rs')
        plan = self.prepare()
        self.run_plan(plan)
        replacement = self.root / 'replacement'
        replacement.write_bytes(path.read_bytes())
        os.replace(replacement, path)
        self.run_plan(plan)

    def test_file_replacement_during_read_is_rejected(self):
        path = self.file('a/source.rs')
        replacement = self.root / 'replacement'
        replacement.write_bytes(path.read_bytes())
        plan = self.prepare()
        original_read = os.read
        replaced = False
        def read(descriptor, count):
            nonlocal replaced
            data = original_read(descriptor, count)
            if not replaced:
                os.replace(replacement, path)
                replaced = True
            return data
        with patch.object(os, 'read', side_effect=read), self.assertRaises(subject.Denied):
            self.run_plan(plan)
        self.assertTrue(replaced)

    def test_cached_plan_does_not_authorize_new_symlink(self):
        path = self.file('a/source.rs')
        plan = self.prepare()
        self.run_plan(plan)
        other = self.root / 'replacement'
        other.write_bytes(path.read_bytes())
        path.unlink()
        path.symlink_to(other)
        with self.assertRaises((subject.Denied, OSError)):
            self.run_plan(plan)

    def test_replaced_ancestor_is_rejected_at_final_check(self):
        self.file('a/source.rs')
        plan = self.prepare()
        original_check = subject._source_read_and_check
        replacement = self.root / 'replacement'
        replacement.mkdir()
        (replacement / 'source.rs').write_bytes(b'original source\n')
        def read_and_replace(*args):
            original_check(*args)
            (self.root / 'a').rename(self.root / 'old')
            replacement.rename(self.root / 'a')
        with patch.object(subject, '_source_read_and_check', side_effect=read_and_replace), \
             self.assertRaisesRegex(subject.Denied, 'ancestor'):
            self.run_plan(plan)

    def test_short_reads_still_hash_the_complete_file(self):
        self.file('source.rs', b'abcdef' * 100)
        plan = self.prepare()
        read = os.read
        with patch.object(os, 'read', side_effect=lambda fd, count: read(fd, min(count, 7))):
            self.run_plan(plan)

    def test_all_open_descriptors_close_on_read_failure(self):
        self.file('a/b/source.rs')
        plan = self.prepare()
        opened, closed = [], []
        original_open, original_close = os.open, os.close
        def open_file(*args, **kwargs):
            fd = original_open(*args, **kwargs)
            opened.append(fd)
            return fd
        def close_file(fd):
            closed.append(fd)
            return original_close(fd)
        with patch.object(os, 'open', side_effect=open_file), \
             patch.object(os, 'close', side_effect=close_file), \
             patch.object(os, 'read', side_effect=OSError('injected read failure')), \
             self.assertRaises(OSError):
            self.run_plan(plan)
        self.assertCountEqual(opened, closed)

    def test_all_open_descriptors_close_on_directory_fstat_failure(self):
        self.file('a/b/source.rs')
        plan = self.prepare()
        opened, closed = [], []
        original_open, original_close, original_fstat = os.open, os.close, os.fstat
        def open_file(*args, **kwargs):
            fd = original_open(*args, **kwargs)
            opened.append(fd)
            return fd
        def close_file(fd):
            closed.append(fd)
            return original_close(fd)
        def fstat(fd):
            if len(opened) == 2:
                raise OSError('injected directory fstat failure')
            return original_fstat(fd)
        with patch.object(os, 'open', side_effect=open_file), \
             patch.object(os, 'close', side_effect=close_file), \
             patch.object(os, 'fstat', side_effect=fstat), self.assertRaises(OSError):
            self.run_plan(plan)
        self.assertCountEqual(opened, closed)

    def test_ancestor_lstat_failure_is_not_cached_away(self):
        self.file('a/source.rs')
        plan = self.prepare()
        self.run_plan(plan)
        original_lstat = Path.lstat
        def lstat(path):
            if path == self.root / 'a':
                raise OSError('injected ancestor lookup failure')
            return original_lstat(path)
        with patch.object(Path, 'lstat', lstat), self.assertRaises(OSError):
            self.run_plan(plan)

    def test_deep_path_fallback_stays_fresh_without_directory_stack(self):
        path = self.file('a/b/c/source.rs', b'abcd')
        original_open = os.open
        directory_opens = []
        def open_file(path, flags, *args, **kwargs):
            if flags & os.O_DIRECTORY:
                directory_opens.append(path)
            return original_open(path, flags, *args, **kwargs)
        with patch.object(subject, '_SOURCE_DIRECTORY_STACK_LIMIT', 1), \
             patch.object(os, 'open', side_effect=open_file):
            with patch.object(Path, 'joinpath', side_effect=AssertionError('planned unused deep traversal')):
                plan = self.prepare()
            self.run_plan(plan)
            path.write_bytes(b'wxyz')
            with self.assertRaises(subject.Denied):
                self.run_plan(plan)
        self.assertEqual(directory_opens, [])

    def test_branch_transitions_and_duplicate_declarations_preserve_visits(self):
        for path in ['a/x/source.rs', 'a-/source.rs', 'a/y/source.rs', 'b.rs', 'z/é space.rs']:
            self.file(path)
        self.entries.extend([dict(self.entries[0]), dict(self.entries[-1])])
        self.entries.reverse()
        expected = sorted(row['path'] for row in self.entries)
        plan = self.prepare()
        actual = []
        original = subject._source_read_and_check
        def visit(path, *args):
            actual.append(str(path))
            return original(path, *args)
        with patch.object(subject, '_source_read_and_check', side_effect=visit):
            self.run_plan(plan)
            self.run_plan(plan)
        self.assertEqual(actual, expected * 2)

    def test_different_execution_worktree_refuses_before_open(self):
        self.file('source.rs')
        plan = self.prepare()
        with patch.object(os, 'open', side_effect=AssertionError('opened wrong worktree')), \
             self.assertRaisesRegex(subject.Denied, 'worktree changed'):
            subject._validate_source_declaration(self.root / 'other', plan)

    def test_directory_is_not_accepted_as_source(self):
        self.file('a/source.rs')
        self.entries = [{'path': str(self.root / 'a'), 'sha256': hashlib.sha256(b'').hexdigest()}]
        with self.assertRaisesRegex(subject.Denied, 'nonregular'):
            self.run_plan(self.prepare())

    def test_empty_and_multichunk_sources_still_validate(self):
        self.file('empty.rs', b'')
        self.file('large.rs', b'abcdef0123456789' * 10000)
        self.run_plan(self.prepare())

    def test_root_worktree_keeps_the_single_root_anchor(self):
        path = self.file('source.rs')
        plan = subject._prepare_source_declaration(Path('/'), self.entries)
        subject._validate_source_declaration(Path('/'), plan)
        path.write_bytes(b'changed')
        with self.assertRaises(subject.Denied):
            subject._validate_source_declaration(Path('/'), plan)


if __name__ == '__main__':
    unittest.main()
