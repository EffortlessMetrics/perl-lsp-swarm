#!/usr/bin/env python3
"""Single-process cohort scope inside an existing cargo-admitted lease.

This module does not acquire/release leases, choose tests, or implement another
supervisor. Invoke it in a dedicated process. Only successful producer and exact
executable consumers followed by kernel closure permit narrowly scoped reclaim.
Private roots must remain exclusively owned; unlink is not an atomic conditional
operation against hostile concurrent writers. Hardlinked Cargo aliases are refused.
"""
import hashlib
import re
import json
import os
import sys
from pathlib import Path
import stat
import subprocess
import signal
import threading

from cargo_admitted import ClippyTree, Denied, directory_identity, native_path, owns_lease, resource_plan


def children_capability():
    """Probe the live calling Linux thread, never another process's proc entry."""
    path = Path('/proc/self/task') / str(threading.get_native_id()) / 'children'
    try:
        data = path.read_text()
        if len(data) > 65536 or any(not token.isdecimal() for token in data.split()):
            raise Denied('invalid owned child table')
    except OSError as error:
        raise Denied('cohort requires readable live-thread proc children: ' + str(path)) from error
    return str(path)


def facts(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns, metadata.st_mode, metadata.st_nlink)


class Cohort:
    """One producer, its consumers, and one settlement; no recovery adoption API."""
    def __init__(self, descriptor, source_digest, profile_digest, *, source_subject, profile_subject, expected_targets, evidence_path, pinned=()):
        self.pid = os.getpid()
        self.thread = threading.get_native_id()
        self.descriptor = json.loads(json.dumps(descriptor))
        for key in ('worktree', 'resources', 'lease', 'lease_identity',
                    'lease_marker', 'marker_identity', 'pid'):
            if key not in self.descriptor:
                raise Denied('missing original lease descriptor field: ' + key)
        for digest in (source_digest, profile_digest):
            if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
                raise Denied('cohort requires source/profile SHA256 bindings')
        self.binding = (source_digest, profile_digest)
        self.subjects = (Path(source_subject), Path(profile_subject))
        self.ancestor = self.descriptor['pid']
        cursor = os.getppid()
        seen = set()
        while cursor != self.ancestor:
            if cursor <= 1 or cursor in seen:
                raise Denied('outer lease owner is not a live ancestor')
            seen.add(cursor)
            cursor = self._process_identity(cursor)[0]
        self.ancestor_identity = self._process_identity(self.ancestor)
        self.worktree = native_path(self.descriptor['worktree'])
        if str(self.worktree) != self.descriptor['worktree'] or Path.cwd().resolve() != self.worktree:
            raise Denied('noncanonical worktree')
        self.resources = {k: native_path(v) for k, v in self.descriptor['resources'].items()}
        for key in ('target', 'build', 'cargo_home', 'temp'):
            if key not in self.resources:
                raise Denied('missing admitted resource: ' + key)
        if any(str(path) != self.descriptor['resources'][key] for key, path in self.resources.items()):
            raise Denied('noncanonical resource')
        self.roots = (self.resources['target'], self.resources['build'])
        admitted_worktree, admitted_slot, admitted_resources = resource_plan(os.environ)
        if admitted_worktree != self.worktree or self.resources != admitted_resources:
            raise Denied('descriptor differs from native admitted resource plan')
        marker = Path(self.descriptor['lease_marker'])
        if Path(self.descriptor['lease']) != admitted_slot / 'cargo-active' or marker.parent != admitted_slot / 'cargo-active' or not re.fullmatch(r'owner-[0-9a-f]{32}', marker.name):
            raise Denied('descriptor differs from native lease/marker location')
        self.evidence_path = Path(evidence_path)
        if not self.evidence_path.is_absolute() or any(self.evidence_path.is_relative_to(root) for root in self.roots):
            raise Denied('evidence must be outside executable roots')
        if native_path(str(self.evidence_path)) != self.evidence_path or os.path.lexists(self.evidence_path):
            raise Denied('evidence path must be canonical and new')
        self.receipt_path = self.evidence_path.with_name(self.evidence_path.name + '.receipt.jsonl')
        if os.path.lexists(self.receipt_path):
            raise Denied('receipt evidence path must be new')
        if not isinstance(expected_targets, list) or len(expected_targets) > 1024:
            raise Denied('cohort requires at most 1024 explicit targets')
        self.expected_targets = {json.dumps(entry, sort_keys=True) for entry in expected_targets}
        if not self.expected_targets or any(entry['target'].get('test') is not True or entry['profile'].get('test') is not True for entry in expected_targets):
            raise Denied('expected targets must be explicit test harnesses')
        self.captured_targets = set()
        self.target_paths = {}
        self.path_targets = {}
        self.worktree_identity = directory_identity(self.worktree)
        self.root_identity = {path: directory_identity(path) for path in self.roots}
        self.pinned = {native_path(str(path)) for path in pinned}
        self.preexisting = {path for root in self.roots for path in root.rglob('*')}
        self.records = {}
        self.used = False
        self.finished = False
        self.receipt = None
        self._cleanup_authorized = False
        self.ancestor_fd = os.pidfd_open(self.ancestor)
        try:
            self._check_owner()
        except BaseException:
            os.close(self.ancestor_fd)
            raise

    @staticmethod
    def _process_identity(pid):
        try:
            fields = (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()
            if fields[0] in ('Z', 'X', 'x'):
                raise Denied('bound outer owner has exited')
            return int(fields[1]), int(fields[19])  # PPID, kernel starttime
        except (OSError, ValueError, IndexError) as error:
            raise Denied('outer owner identity unavailable') from error

    def _check_owner(self):
        if os.getpid() != self.pid or threading.get_native_id() != self.thread:
            raise Denied('cohort ownership cannot transfer across process/thread')
        if self._process_identity(self.ancestor) != self.ancestor_identity:
            raise Denied('outer owner PID identity changed')
        try:
            signal.pidfd_send_signal(self.ancestor_fd, 0)
        except OSError as error:
            raise Denied('bound outer owner no longer live') from error
        for path, digest in zip(self.subjects, self.binding):
            try:
                if not path.is_absolute() or native_path(str(path)) != path:
                    raise ValueError('noncanonical manifest')
                manifest_fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
                try:
                    before = os.fstat(manifest_fd)
                    if not stat.S_ISREG(before.st_mode) or before.st_size > 8 * 1024 * 1024:
                        raise ValueError('manifest must be bounded regular file')
                    manifest = bytearray()
                    while len(manifest) <= 8 * 1024 * 1024:
                        chunk = os.read(manifest_fd, min(65536, 8 * 1024 * 1024 + 1 - len(manifest)))
                        if not chunk:
                            break
                        manifest.extend(chunk)
                    if facts(before) != facts(os.fstat(manifest_fd)) or facts(before) != facts(path.lstat()) or hashlib.sha256(manifest).hexdigest() != digest:
                        raise ValueError('manifest changed')
                finally:
                    os.close(manifest_fd)
                entries = json.loads(manifest)['files']
                if not isinstance(entries, list) or not entries:
                    raise ValueError('empty source/profile manifest')
                for entry in entries:
                    subject = Path(entry['path'])
                    if not subject.is_absolute() or not subject.is_relative_to(self.worktree) or native_path(str(subject)) != subject:
                        raise ValueError('foreign source/profile path')
                    fd = os.open(subject, os.O_RDONLY | os.O_NOFOLLOW)
                    try:
                        before = os.fstat(fd)
                        if not stat.S_ISREG(before.st_mode):
                            raise ValueError('nonregular source/profile subject')
                        observed = hashlib.sha256()
                        while chunk := os.read(fd, 65536):
                            observed.update(chunk)
                        if facts(before) != facts(os.fstat(fd)) or facts(before) != facts(subject.lstat()) or observed.hexdigest() != entry['sha256']:
                            raise ValueError('source/profile file changed')
                    finally:
                        os.close(fd)
            except (OSError, ValueError, KeyError, TypeError) as error:
                raise Denied('source/profile manifest invalid or changed') from error
        d = self.descriptor
        if not owns_lease(Path(d['lease']), tuple(d['lease_identity']),
                          Path(d['lease_marker']), tuple(d['marker_identity'])):
            raise Denied('original outer lease ownership lost')
        if native_path(str(self.worktree)) != self.worktree or directory_identity(self.worktree) != self.worktree_identity:
            raise Denied('worktree identity changed')
        for root, identity in self.root_identity.items():
            if directory_identity(root) != identity:
                raise Denied('private root identity changed')

    def _open(self, path):
        self._check_owner()
        path = Path(path)
        if not path.is_absolute() or path in self.pinned or path in self.preexisting:
            raise Denied('pinned/preexisting/nonabsolute executable refused: ' + str(path))
        if not any(path.is_relative_to(root) and path != root for root in self.roots):
            raise Denied('foreign executable refused: ' + str(path))
        # Walk every component through canonical-path validation, then hold the
        # parent directory fd and bind its identity for the final relative unlink.
        if native_path(str(path)) != path:
            raise Denied('noncanonical executable refused')
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            parent_identity = directory_identity(path.parent)
            if (os.fstat(parent).st_dev, os.fstat(parent).st_ino) != parent_identity:
                raise Denied('executable parent changed')
            fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=parent)
            try:
                before = os.fstat(fd)
                if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not before.st_mode & 0o111:
                    raise Denied('executable must be regular, executable, and singly linked')
                digest = hashlib.sha256()
                while True:
                    chunk = os.read(fd, 65536)
                    if not chunk:
                        break
                    digest.update(chunk)
                after = os.fstat(fd)
                if facts(before) != facts(after) or facts(os.stat(path.name, dir_fd=parent, follow_symlinks=False)) != facts(after):
                    raise Denied('executable changed during observation')
                return parent, parent_identity, facts(after), digest.hexdigest()
            finally:
                os.close(fd)
        except BaseException:
            os.close(parent)
            raise

    def _record(self, message):
        if message.get('reason') != 'compiler-artifact' or not message.get('executable'):
            return
        target_binding = {key: message.get(key) for key in ('package_id', 'target', 'features', 'profile')}
        target_key = json.dumps(target_binding, sort_keys=True)
        if target_key not in self.expected_targets:
            return  # build scripts/public products never acquire cleanup authority
        if message.get('fresh') is not False:
            raise Denied('only newly produced compiler executables may be reclaimed')
        path = Path(message['executable'])
        if target_key in self.target_paths and self.target_paths[target_key] != path:
            raise Denied('one target produced multiple executable paths')
        if path in self.path_targets and self.path_targets[path] != target_key:
            raise Denied('multiple targets produced one executable path')
        parent, parent_identity, identity, digest = self._open(path)
        os.close(parent)
        record = (parent_identity, identity, digest, self.binding)
        if path in self.records and self.records[path] != record:
            raise Denied('compiler executable identity changed')
        self.records[path] = record
        self.target_paths[target_key] = path
        self.path_targets[path] = target_key
        self.captured_targets.add(target_key)

    def run(self, producer, consumer_args, env, *, settlement_timeout=30):
        """Run exact supplied commands; consumer_args maps every executable to argv.

        Producer stdout must be Cargo JSON; diagnostics are copied unchanged to
        the caller's stdout. Failures retain all artifacts. No caller-supplied
        settlement/consumption flags or persisted receipt can authorize deletion.
        """
        if self.used:
            raise Denied('cohort cannot run twice')
        self.used = True
        try:
            self._check_owner()
            children_capability()  # fail before any resource-bearing process
            tree = ClippyTree()
        except BaseException:
            self.close()
            raise
        status = None
        error = None
        consumed = []
        try:
            child_env = dict(env)
            expected = {'CARGO_TARGET_DIR': self.resources['target'],
                        'CARGO_BUILD_BUILD_DIR': self.resources['build'],
                        'CARGO_HOME': self.resources['cargo_home'], 'TMPDIR': self.resources['temp']}
            for key, path in expected.items():
                if child_env.get(key) != str(path):
                    raise Denied('producer environment differs from admitted resource: ' + key)
            # A private disk spool preserves diagnostics without pipe deadlocks or
            # accumulating compiler output in memory. It remains retained evidence.
            with self.evidence_path.open('xb+') as output:
                status = self._launch(tree, producer, child_env, stdout=output)
                output.seek(0)
                for line in output:
                    sys.stdout.buffer.write(line)
                    if status == 0:
                        try:
                            message = json.loads(line)
                        except (ValueError, UnicodeError) as exception:
                            raise Denied('producer stdout is not Cargo JSON') from exception
                        self._record(message)
                output.flush()
                os.fsync(output.fileno())
            if status == 0:
                if self.captured_targets != self.expected_targets or not self.records or {str(path) for path in self.records} != set(consumer_args):
                    raise Denied('consumer coverage differs from captured executables')
                for path in self.records:
                    parent, _, identity, digest = self._open(path)
                    os.close(parent)
                    if (identity, digest) != self.records[path][1:3]:
                        raise Denied('executable changed before consumption')
                    status = self._launch(tree, [str(path), *consumer_args[str(path)]], child_env)
                    if status:
                        break
                    consumed.append(str(path))
        except BaseException as exception:
            error = exception
        try:
            self.receipt = tree.finish(stop=error is not None, timeout=settlement_timeout)
            self.finished = True
            reclaimed = []
            if error is None and status == 0 and self.receipt['tree_settled'] and not self.receipt['cancelled']:
                # Validate all before deleting any; subsequent per-path validation
                # still fails closed if a cooperative owner contract is violated.
                for path in self.records:
                    parent, parent_identity, identity, digest = self._open(path)
                    os.close(parent)
                    if (parent_identity, identity, digest, self.binding) != self.records[path]:
                        raise Denied('executable changed before reclaim')
                with self.receipt_path.open('xb') as evidence:
                    authorization = {'event': 'authorization', 'product_status': status,
                                     'settlement': self.receipt, 'consumed': consumed,
                                     'source_profile_binding': self.binding,
                                     'records': [{'path': str(path), 'parent_identity': record[0],
                                                  'file_identity': record[1], 'sha256': record[2],
                                                  'binding': record[3]} for path, record in self.records.items()]}
                    self._persist(evidence, authorization)
                    # New file names need directory durability as well as data.
                    evidence_parent = os.open(self.receipt_path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                    try:
                        os.fsync(evidence_parent)
                    finally:
                        os.close(evidence_parent)
                    self._cleanup_authorized = True
                    for path in list(self.records):
                        self._reclaim(path)
                        reclaimed.append(str(path))
                        # Unlink and receipt append are not one transaction. An
                        # append failure stops further deletion, never rolls back.
                        self._persist(evidence, {'event': 'reclaimed', 'path': str(path)})
            self._cleanup_authorized = False
            if error is not None:
                self.receipt['error'] = {'type': type(error).__name__, 'message': str(error)}
                if isinstance(error, KeyboardInterrupt):
                    self.receipt['interruption'] = str(error)
            # Unproven closure owns failure precedence even when parsing or
            # validation failed first. Keep its cause in the returned settlement.
            effective = 75 if not self.receipt['tree_settled'] else (130 if self.receipt['cancelled'] or isinstance(error, KeyboardInterrupt) else status)
            if error is not None and self.receipt['tree_settled'] and not isinstance(error, KeyboardInterrupt):
                raise error
            return {'status': effective, 'product_status': status, 'evidence': str(self.evidence_path), 'authorization_evidence': str(self.receipt_path) if self.receipt_path.exists() else None, 'settlement': self.receipt, 'consumed': consumed,
                    'reclaimed': reclaimed, 'binding': self.binding}
        finally:
            self._cleanup_authorized = False
            self.close()

    @staticmethod
    def _persist(stream, payload):
        encoded = (json.dumps(payload, sort_keys=True) + '\n').encode('utf-8')
        if len(encoded) > 8 * 1024 * 1024:
            raise Denied('cohort evidence exceeds bounded receipt size')
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())

    def close(self):
        """Close only the ancestor observation; never release lease/resources."""
        if self.ancestor_fd is not None:
            os.close(self.ancestor_fd)
            self.ancestor_fd = None

    def _launch(self, tree, command, env, **kwargs):
        self._check_owner()
        tree.binding = True
        try:
            process = subprocess.Popen(command, env=env, cwd=self.worktree, **kwargs)
            tree.track(process.pid, process)
        finally:
            tree.binding = False
        if tree.cancelled:
            raise KeyboardInterrupt('cohort cancelled after child binding')
        return process.wait()

    def _reclaim(self, path):
        if not self._cleanup_authorized or path not in self.records or not self.finished or self.receipt is None or not self.receipt['tree_settled']:
            raise Denied('cohort has no kernel closure')
        parent, parent_identity, identity, digest = self._open(path)
        try:
            if (parent_identity, identity, digest, self.binding) != self.records[path]:
                raise Denied('executable identity changed before unlink')
            if directory_identity(path.parent) != parent_identity:
                raise Denied('executable parent replaced before unlink')
            if facts(os.stat(path.name, dir_fd=parent, follow_symlinks=False)) != identity:
                raise Denied('executable replaced before unlink')
            os.unlink(path.name, dir_fd=parent)
            del self.records[path]  # no repeat/replacement cleanup authority
        finally:
            os.close(parent)
