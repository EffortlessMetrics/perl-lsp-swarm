#!/usr/bin/env python3
"""Single-process cohort scope inside an existing cargo-admitted lease.

This module does not acquire/release leases, choose tests, or implement another
supervisor. Invoke it in a dedicated process. Only successful producer and exact
executable consumers followed by kernel closure permit narrowly scoped reclaim.
Private roots must remain exclusively owned; unlink is not an atomic conditional
operation against hostile concurrent writers. Only complete, newly produced private Cargo alias groups may be reclaimed.
"""
import hashlib
from collections import Counter
import re
import json
import os
import sys
from pathlib import Path
import stat
import subprocess
import signal
import threading
import time
from contextlib import contextmanager, nullcontext

from cargo_admitted import ClippyTree, Denied, directory_identity, native_path, owns_lease, resource_plan, check_capacity, COHORT_RUNTIME_KEYS, child_discovery_capability


def children_capability(mode=None):
    """Probe the live calling Linux thread, never another process's proc entry."""
    mode = os.environ.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children') if mode is None else mode
    if mode != 'proc-children':
        return child_discovery_capability(mode)
    path = Path('/proc/self/task') / str(threading.get_native_id()) / 'children'
    try:
        data = path.read_text()
        if len(data) > 65536 or any(not token.isdecimal() for token in data.split()):
            raise Denied('invalid owned child table')
    except OSError as error:
        raise Denied('cohort requires readable live-thread proc children: ' + str(path)) from error
    return str(path)


def source_path_is_canonical(path):
    """Fresh Linux manifest-path validation, with one stat per current component.

    All subjects are already absolute. Visit every ancestor on every call; no
    directory or source facts survive into another file, launch or reclaim.
    Avoid native_path's repeated link stats and second resolve traversal while
    preserving its rejection of links, junctions and noncanonical spellings.
    """
    value = str(path)
    if (not path.is_absolute() or '..' in path.parts or value.startswith(('//', '/mnt/'))
            or '{' in value or '}' in value):
        return False
    for component in (path, *path.parents):
        metadata = component.lstat()
        if (stat.S_ISLNK(metadata.st_mode)
                or getattr(metadata, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0)):
            return False
    return True


def facts(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns, metadata.st_mode, metadata.st_nlink)


def libtest_text_without_rpc_frames(raw):
    """Separate exact byte-framed RPC output from libtest's completion lines.

    Keep the original log separately. Never count completion-shaped strings in
    JSON bodies, or recover from a malformed frame by skipping arbitrary text.
    Only an anchored unfinished libtest prefix permits an inline frame; keep
    boundary separation elsewhere so removal cannot manufacture a new line.
    """
    output = bytearray()
    cursor = 0
    boundary = True
    line_start = 0
    while cursor < len(raw):
        pending = False
        if raw.startswith(b'Content-Length:', cursor):
            pending = (not boundary and re.fullmatch(
                rb'test [^\r\n]+ \.\.\. ', output[line_start:]) is not None)
        if (boundary or pending) and raw.startswith(b'Content-Length:', cursor):
            end = raw.find(b'\r\n\r\n', cursor)
            if end < 0:
                raise Denied('malformed native JSON-RPC frame header')
            headers = raw[cursor:end].split(b'\r\n')
            length = re.fullmatch(rb'Content-Length: ([0-9]+)', headers[0])
            if (length is None or len(headers) > 2
                    or (len(headers) == 2 and re.fullmatch(rb'Content-Type: [ -~]+', headers[1]) is None)):
                raise Denied('malformed native JSON-RPC frame header')
            size = int(length.group(1))
            start = end + 4
            if size <= 0 or size > len(raw) - start:
                raise Denied('truncated native JSON-RPC frame body')
            def unique_object(pairs):
                value = {}
                for key, item in pairs:
                    if key in value:
                        raise ValueError('duplicate JSON key')
                    value[key] = item
                return value
            try:
                message = json.loads(raw[start:start + size].decode('utf-8'), object_pairs_hook=unique_object,
                                     parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))
            except (ValueError, UnicodeError) as error:
                raise Denied('malformed native JSON-RPC frame body') from error
            if (not isinstance(message, dict) or message.get('jsonrpc') != '2.0'
                    or ('id' in message and (isinstance(message['id'], bool)
                        or not isinstance(message['id'], (str, int, type(None)))))
                    or not ((isinstance(message.get('method'), str)
                             and 'result' not in message and 'error' not in message)
                            or ('method' not in message and 'id' in message
                                and (('result' in message) != ('error' in message))))):
                raise Denied('unsupported native JSON-RPC frame envelope')
            if ('params' in message and not isinstance(message['params'], (dict, list))):
                raise Denied('unsupported native JSON-RPC frame params')
            if 'error' in message:
                error = message['error']
                if (not isinstance(error, dict) or isinstance(error.get('code'), bool)
                        or not isinstance(error.get('code'), int) or not isinstance(error.get('message'), str)):
                    raise Denied('unsupported native JSON-RPC frame error')
            cursor = start + size
            if not pending:
                output.extend(b'\n')
                line_start = len(output)
            boundary = not pending
        else:
            byte = raw[cursor]
            output.append(byte)
            cursor += 1
            boundary = byte == 10
            if boundary:
                line_start = len(output)
    return output.decode('utf-8')


_SOURCE_DIRECTORY_STACK_LIMIT = 128


def _source_reject_link(metadata):
    if (stat.S_ISLNK(metadata.st_mode)
            or getattr(metadata, 'st_file_attributes', 0)
            & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0)):
        raise Denied('linked or reparse source component')


def _source_directory_key(metadata):
    _source_reject_link(metadata)
    if not stat.S_ISDIR(metadata.st_mode):
        raise Denied('source ancestor is not a directory')
    return metadata.st_dev, metadata.st_ino, stat.S_IFMT(metadata.st_mode)


def _source_read_and_check(path, expected_hash, directory_fd=None):
    descriptor = os.open(path if directory_fd is None else path.name,
                         os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory_fd)
    try:
        before = os.fstat(descriptor)
        _source_reject_link(before)
        if not stat.S_ISREG(before.st_mode):
            raise Denied('nonregular source/profile subject')
        observed = hashlib.sha256()
        while chunk := os.read(descriptor, 65536):
            observed.update(chunk)
        after = os.fstat(descriptor)
        current = path.lstat()
        _source_reject_link(after)
        _source_reject_link(current)
        if (facts(before) != facts(after) or facts(before) != facts(current)
                or observed.hexdigest() != expected_hash):
            raise Denied('source/profile bytes or current identity changed')
    finally:
        os.close(descriptor)


def _source_verify_directory_names(bindings):
    for path, expected in sorted(bindings.items(), key=lambda item: len(item[0].parts)):
        if _source_directory_key(path.lstat()) != expected:
            raise Denied('source ancestor current name/type/identity changed')


def _prepare_source_declaration(worktree, entries):
    """Plan lexical traversal once; never retain observed filesystem facts."""
    worktree = Path(worktree)
    root_text = str(worktree)
    if (not worktree.is_absolute() or root_text.startswith(('//', '/mnt/'))
            or '..' in worktree.parts or '{' in root_text or '}' in root_text):
        raise Denied('noncanonical worktree spelling')
    subjects = []
    deepest = len(worktree.parts)
    for entry in entries:
        subject = Path(entry['path'])
        value = str(subject)
        if (not subject.is_relative_to(worktree) or subject == worktree
                or not subject.is_absolute() or value.startswith(('//', '/mnt/'))
                or '..' in subject.parts or '{' in value or '}' in value):
            raise Denied('foreign or noncanonical source/profile path')
        subjects.append((subject, entry['sha256']))
        deepest = max(deepest, len(subject.parent.parts))
    # The deep-path fallback never replays a directory stack. Avoid building
    # unused ancestor paths or transitions for that subject.
    if deepest >= _SOURCE_DIRECTORY_STACK_LIMIT:
        return root_text, tuple(subjects), deepest, (), ()
    root_paths = tuple(reversed(worktree.parents)) + (worktree,)
    steps = []
    previous = ()
    root_prefix = root_text.rstrip('/') + '/'
    for subject, expected_hash in sorted(subjects, key=lambda row: str(row[0])):
        parents = tuple(part for part in str(subject)[len(root_prefix):].split('/')
                        if part not in ('', '.'))[:-1]
        common = 0
        while common < min(len(previous), len(parents)) and previous[common] == parents[common]:
            common += 1
        descend = tuple((parents[index], worktree.joinpath(*parents[:index + 1]))
                        for index in range(common, len(parents)))
        steps.append((subject, expected_hash, len(previous) - common, descend))
        previous = parents
    return root_text, tuple(subjects), deepest, root_paths, tuple(steps)


def validate_source_entries(worktree, entries):
    """Prepare the caller's declaration and validate all filesystem facts afresh."""
    _validate_source_declaration(worktree, _prepare_source_declaration(worktree, entries))


def _validate_source_declaration(worktree, entries):
    """Replay lexical traversal, opening and checking every subject afresh.

    The plan retains names and traversal steps only. File/directory descriptors,
    observed identities/hashes and ancestor bindings belong to this invocation. Deep
    paths retain the existing per-file validation before opening a stack.
    """
    root_text, subjects, deepest, root_paths, steps = entries
    worktree = Path(worktree)
    if root_text != str(worktree):
        raise Denied('prepared declaration worktree changed')
    if deepest >= _SOURCE_DIRECTORY_STACK_LIMIT:
        for subject, expected_hash in subjects:
            if not source_path_is_canonical(subject):
                raise Denied('foreign or noncanonical source/profile path')
            _source_read_and_check(subject, expected_hash)
        return

    stack = []
    bindings = {}
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        current = root_paths[0]
        descriptor = os.open('/', directory_flags)
        stack.append((current, descriptor))  # register before even fstat can fail
        bindings[current] = _source_directory_key(os.fstat(descriptor))
        for current in root_paths[1:]:
            descriptor = os.open(current.name, directory_flags, dir_fd=stack[-1][1])
            try:
                observed = _source_directory_key(os.fstat(descriptor))
                if _source_directory_key(current.lstat()) != observed:
                    raise Denied('source root component replaced')
            except BaseException:
                os.close(descriptor)
                raise
            stack.append((current, descriptor))
            bindings[current] = observed
        for subject, expected_hash, ascend, descend in steps:
            for _ in range(ascend):
                os.close(stack.pop()[1])
            for name, current in descend:
                descriptor = os.open(name, directory_flags, dir_fd=stack[-1][1])
                try:
                    observed = _source_directory_key(os.fstat(descriptor))
                    if current in bindings and bindings[current] != observed:
                        raise Denied('source ancestor rebound during validation')
                    if _source_directory_key(current.lstat()) != observed:
                        raise Denied('source ancestor current identity differs from anchor')
                except BaseException:
                    os.close(descriptor)
                    raise
                stack.append((current, descriptor))
                bindings[current] = observed
            _source_read_and_check(subject, expected_hash, stack[-1][1])
        _source_verify_directory_names(bindings)
    finally:
        while stack:
            os.close(stack.pop()[1])


class PhaseJournal:
    """Bounded durable observations, never settlement or cleanup authority."""
    MAX_BYTES = 8 * 1024 * 1024
    MAX_RECORDS = 65536

    def __init__(self, request_path, request_bytes, request, descriptor):
        evidence = Path(request['evidence_path'])
        if (not evidence.is_absolute() or native_path(str(evidence)) != evidence
                or evidence.parent != request_path.parent
                or any(evidence.is_relative_to(Path(value)) for key, value in descriptor['resources'].items() if key in ('target', 'build'))):
            raise Denied('journal evidence must be canonical, beside original request and outside executable roots')
        self.path = evidence.with_name(evidence.name + '.phases.jsonl')
        self.sequence = 0; self.bytes = 0; self.stack = []; self.fd = None; self.failure = None
        binding = {'request_sha256': hashlib.sha256(request_bytes).hexdigest(),
                   'evidence_path': str(evidence), 'source_digest': request['source_digest'],
                   'profile_digest': request['profile_digest'], 'expected_targets': request['expected_targets'],
                   'original_descriptor': descriptor,
                   'core_source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
        self.binding = hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()
        self.fd = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            self._write('header', binding=binding, advisory_only=True)
            parent = os.open(self.path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try: os.fsync(parent)
            finally: os.close(parent)
        except BaseException:
            self.close(); raise

    def _write(self, event, **fields):
        row = {'version': 1, 'sequence': self.sequence, 'monotonic_ns': time.monotonic_ns(),
               'binding_sha256': self.binding, 'event': event, **fields}
        if '_duration_start_ns' in fields:
            row.pop('_duration_start_ns')
            row['duration_ns'] = row['monotonic_ns'] - fields['_duration_start_ns']
        encoded = (json.dumps(row, sort_keys=True) + '\n').encode()
        if self.sequence >= self.MAX_RECORDS or self.bytes + len(encoded) > self.MAX_BYTES:
            raise Denied('phase journal bound exceeded')
        cursor = 0
        while cursor < len(encoded):
            written = os.write(self.fd, encoded[cursor:])
            if written <= 0: raise OSError('phase journal short write')
            cursor += written
        os.fsync(self.fd)
        self.sequence += 1; self.bytes += len(encoded)
        return row

    def begin(self, phase, **context):
        token = {'phase_id': self.sequence, 'phase': phase, 'started_ns': time.monotonic_ns()}
        row = self._write('started', phase_id=token['phase_id'], phase=phase, context=context)
        token['started_ns'] = row['monotonic_ns']
        self.stack.append(token)
        return token

    def end(self, token, outcome='returned', error=None):
        if not self.stack or self.stack[-1] != token:
            raise Denied('phase journal nesting changed')
        self._write('completed', phase_id=token['phase_id'], phase=token['phase'],
                    _duration_start_ns=token['started_ns'], outcome=outcome, error=error, exit_status=token.get('exit_status'))
        self.stack.pop()

    @contextmanager
    def phase(self, phase, **context):
        token = self.begin(phase, **context)
        try:
            yield token
        except BaseException as error:
            try:
                self.end(token, 'raised', {'type': type(error).__name__, 'message': str(error)[:1024]})
            except BaseException as persistence_error:
                # Preserve the original interruption/error and its existing
                # settlement precedence; progress cannot override that status.
                self.failure = {'type': type(persistence_error).__name__, 'message': str(persistence_error)[:1024]}
            raise
        else:
            self.end(token)

    def child(self, process):
        # A fast child may already be a zombie. Observational starttime permits
        # that state; it never substitutes for existing leader wait ownership.
        identity = {'pid': process.pid, 'kernel_starttime': None, 'state': 'NOT_OBSERVED'}
        try:
            fields = Path('/proc/{}/stat'.format(process.pid)).read_text().rsplit(')', 1)[1].split()
            identity.update(kernel_starttime=int(fields[19]), state=fields[0])
        except (OSError, ValueError, IndexError):
            pass
        self._write('child', phase_id=self.stack[-1]['phase_id'], **identity)

    def close(self):
        if self.fd is not None:
            os.close(self.fd); self.fd = None


class Cohort:
    """One producer, its consumers, and one settlement; no recovery adoption API."""
    def __init__(self, descriptor, source_digest, profile_digest, *, source_subject, profile_subject, expected_targets, evidence_path, pinned=(), pinned_subjects=None, retain_preexisting=False, journal=None):
        if not isinstance(retain_preexisting, bool):
            raise Denied("retained consumption mode must be boolean")
        self.retain_preexisting = retain_preexisting
        self.journal = journal
        self.pid = os.getpid()
        self.thread = threading.get_native_id()
        self.descriptor = json.loads(json.dumps(descriptor))
        self.child_discovery = self.descriptor.get('child_discovery', 'proc-children')
        if self.child_discovery != os.environ.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children'):
            raise Denied('cohort discovery profile differs from admitted environment')
        for key in ('worktree', 'resources', 'lease', 'lease_identity',
                    'lease_marker', 'marker_identity', 'pid', 'admission'):
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
        if any(entry['target'].get('test') is not True or entry['profile'].get('test') is not True for entry in expected_targets):
            raise Denied('expected targets must be explicit test harnesses')
        self.captured_targets = set()
        self.target_paths = {}
        self.path_targets = {}
        self.binary_artifacts = {}
        self.worktree_identity = directory_identity(self.worktree)
        self.root_identity = {path: directory_identity(path) for path in self.roots}
        self.pinned = {native_path(str(path)) for path in pinned}
        self.pinned_subjects = {native_path(str(path)): digest for path, digest in (pinned_subjects or {}).items()}
        if not self.pinned_subjects.keys() <= self.pinned:
            raise Denied('pinned hash subjects must be explicit pins')
        self.preexisting = {path for root in self.roots for path in root.rglob('*')}
        self.records = {}
        self.groups = {}
        self.build_scripts = []
        self._harness_sequence = 0
        self._receipt_stream = None
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

    def _pin_validation(self):
        for path, expected_subject in self.pinned_subjects.items():
            if native_path(str(path)) != path:
                raise Denied('pinned product path is no longer canonical')
            expected_hash = expected_subject['sha256'] if isinstance(expected_subject, dict) else expected_subject
            expected_identity = expected_subject.get('file_identity') if isinstance(expected_subject, dict) else None
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            try:
                before = os.fstat(fd)
                if not stat.S_ISREG(before.st_mode):
                    raise Denied('pinned product is no longer regular')
                digest = hashlib.sha256()
                while chunk := os.read(fd, 65536):
                    digest.update(chunk)
                if expected_identity is not None and list(facts(before)) != expected_identity:
                    raise Denied('pinned public executable identity changed')
                if digest.hexdigest() != expected_hash or facts(before) != facts(os.fstat(fd)) or facts(before) != facts(path.lstat()):
                    raise Denied('pinned public executable changed')
            finally:
                os.close(fd)

    def _source_validation(self):
        for path, digest in zip(self.subjects, self.binding):
            try:
                if not source_path_is_canonical(path):
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
                # Reuse only declaration geometry after freshly authenticating the
                # complete current manifest above. Source bytes, identities and
                # ancestor bindings are checked again below on every cache hit.
                cache = getattr(self, '_authenticated_declarations', None)
                if cache is None:
                    cache = self._authenticated_declarations = {}
                key = (str(path), digest, str(self.worktree))
                entries = cache.get(key)
                if entries is None:
                    parsed = json.loads(manifest)['files']
                    if not isinstance(parsed, list) or not parsed:
                        raise ValueError('empty source/profile manifest')
                    entries = _prepare_source_declaration(self.worktree, parsed)
                    cache[key] = entries
                _validate_source_declaration(self.worktree, entries)
            except (OSError, ValueError, KeyError, TypeError) as error:
                raise Denied('source/profile manifest invalid or changed') from error

    def _phase(self, phase, **context):
        journal = getattr(self, 'journal', None)
        return journal.phase(phase, **context) if journal is not None else nullcontext()

    def _check_owner(self):
        with self._phase('owner_validation'):
            self._check_owner_impl()

    def _check_owner_impl(self):
        if (self.descriptor.get('child_discovery', 'proc-children') != self.child_discovery
                or os.environ.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children') != self.child_discovery):
            raise Denied('cohort discovery profile binding changed')
        if os.getpid() != self.pid or threading.get_native_id() != self.thread:
            raise Denied('cohort ownership cannot transfer across process/thread')
        if self._process_identity(self.ancestor) != self.ancestor_identity:
            raise Denied('outer owner PID identity changed')
        try:
            signal.pidfd_send_signal(self.ancestor_fd, 0)
        except OSError as error:
            raise Denied('bound outer owner no longer live') from error
        with self._phase('pin_validation'):
            self._pin_validation()
        with self._phase('source_validation'):
            self._source_validation()
        d = self.descriptor
        runtime = d.get('runtime_environment')
        if runtime is not None:
            if not isinstance(runtime, dict) or not runtime.keys() <= set(COHORT_RUNTIME_KEYS) or any(value is not None and not isinstance(value, str) for value in runtime.values()):
                raise Denied('original runtime environment contains unsupported keys or value types')
            digest = hashlib.sha256(json.dumps(runtime, sort_keys=True).encode()).hexdigest()
            if digest != d.get('runtime_environment_sha256'):
                raise Denied('original runtime environment digest differs from descriptor')
        if not owns_lease(Path(d['lease']), tuple(d['lease_identity']),
                          Path(d['lease_marker']), tuple(d['marker_identity'])):
            raise Denied('original outer lease ownership lost')
        if native_path(str(self.worktree)) != self.worktree or directory_identity(self.worktree) != self.worktree_identity:
            raise Denied('worktree identity changed')
        for root, identity in self.root_identity.items():
            if directory_identity(root) != identity:
                raise Denied('private root identity changed')


    def _open(self, path, *, allow_links=False, allow_preexisting=False):
        self._check_owner()
        path = Path(path)
        if not path.is_absolute() or path in self.pinned or (path in self.preexisting and not allow_preexisting):
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
                if not stat.S_ISREG(before.st_mode) or (before.st_nlink != 1 and not allow_links) or not before.st_mode & 0o111:
                    raise Denied('executable must be regular, executable, and owned as a complete alias group')
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

    def _capture_group(self, path, identity, digest, *, reclaimable=True):
        """Discover exact inode names in both roots; never infer foreign aliases."""
        aliases = []
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        parents = []
        try:
            if facts(os.fstat(fd)) != identity:
                raise Denied('alias inode changed before capture')
            # A freshly observed nlink of one has exactly one inode name.
            # Reopen that precise path through every existing owner/path/hash
            # check; held-fd facts below still reject intervening link changes.
            candidates = ((path,) if identity[-1] == 1 else
                          (candidate for root in self.roots for candidate in root.rglob('*')))
            for candidate in candidates:
                metadata = candidate.lstat()
                if (metadata.st_dev, metadata.st_ino) != identity[:2]:
                    continue
                parent, parent_identity, observed, observed_hash = self._open(candidate, allow_links=True, allow_preexisting=self.retain_preexisting)
                parents.append(parent)
                if observed != identity or observed_hash != digest:
                    raise Denied('alias group changed during capture')
                aliases.append({'path': candidate, 'parent': parent, 'parent_identity': parent_identity})
            if len(aliases) != identity[-1] or not aliases:
                raise Denied('alias count differs from kernel nlink; foreign or unobserved alias')
            if facts(os.fstat(fd)) != identity:
                raise Denied('alias inode changed during capture')
            if any(group['identity'][:2] == identity[:2] for group in self.groups.values()):
                raise Denied('one inode cannot authorize two test targets')
            return {'fd': fd, 'identity': identity, 'initial_identity': identity,
                    'sha256': digest, 'aliases': aliases, 'remaining': list(aliases),
                    'reclaimable': reclaimable and all(alias['path'] not in self.preexisting for alias in aliases)}
        except BaseException:
            os.close(fd)
            for parent in parents:
                os.close(parent)
            raise

    def _validate_group(self, group):
        self._check_owner()
        self._validate_group_observation(group)

    def _validate_group_observation(self, group):
        expected = group['identity']
        if facts(os.fstat(group['fd'])) != expected or expected[-1] != len(group['remaining']):
            raise Denied('owned alias inode or kernel link count changed')
        for alias in group['remaining']:
            path = alias['path']
            if path in self.pinned or (path in self.preexisting and group['reclaimable']) or native_path(str(path)) != path:
                raise Denied('alias became pinned/preexisting/noncanonical')
            parent = alias['parent']
            if directory_identity(path.parent) != alias['parent_identity'] or (os.fstat(parent).st_dev, os.fstat(parent).st_ino) != alias['parent_identity']:
                raise Denied('alias parent identity changed')
            if facts(os.stat(path.name, dir_fd=parent, follow_symlinks=False)) != expected:
                raise Denied('owned alias path replaced or modified')
        # Held descriptor and metadata bind the original inode; readback also
        # detects content changes on filesystems with coarse timestamp behavior.
        os.lseek(group['fd'], 0, os.SEEK_SET)
        observed = hashlib.sha256()
        while chunk := os.read(group['fd'], 65536):
            observed.update(chunk)
        if observed.hexdigest() != group['sha256'] or facts(os.fstat(group['fd'])) != expected:
            raise Denied('owned alias bytes changed')

    def _validate_record(self, path):
        with self._phase('record_validation', executable=str(path), target=getattr(self, 'path_targets', {}).get(path)):
            return self._validate_record_impl(path)

    def _validate_record_impl(self, path):
        # Paired record sites observe every alias before the unchanged _open
        # performs a fresh owner/source/pin and primary-image validation.
        self._validate_group_observation(self.groups[path])
        parent, parent_identity, identity, digest = self._open(
            path, allow_links=True, allow_preexisting=self.retain_preexisting)
        try:
            return parent_identity, identity, digest, self.binding
        finally:
            os.close(parent)

    def _record(self, message):
        target = {key: message.get(key) for key in ('package_id','target','features','profile')}
        if (message.get('reason') == 'compiler-artifact' and message.get('executable')
                and json.dumps(target, sort_keys=True) in self.expected_targets):
            with self._phase('artifact_capture', executable=message['executable'], target=target):
                return self._record_impl(message)
        return self._record_impl(message)

    def _record_impl(self, message):
        if message.get('reason') == 'build-script-executed':
            self.build_scripts.append({key: message.get(key) for key in ('package_id', 'env', 'linked_paths', 'out_dir')})
            return
        if message.get('reason') != 'compiler-artifact':
            return
        executable = message.get('executable')
        target = message.get('target') or {}
        if executable and target.get('kind') == ['bin']:
            package_id = message.get('package_id')
            name = target.get('name')
            if not isinstance(package_id, str) or not isinstance(name, str) or not name or '/' in name or '\\' in name:
                raise Denied('Cargo binary artifact lacks a safe package/target identity')
            path = Path(executable)
            if not path.is_absolute() or native_path(str(path)) != path or not path.is_file() or path.is_symlink():
                raise Denied('Cargo binary artifact path is not canonical and present')
            profile = message.get('profile')
            if not isinstance(profile, dict) or not isinstance(profile.get('test'), bool):
                raise Denied('Cargo binary artifact lacks an exact test profile')
            # A batch can emit both the bin's test harness and its public
            # executable. Only Cargo's non-test artifact supplies BIN_EXE;
            # the test artifact still passes through exact harness binding.
            if profile['test'] is False:
                self.binary_artifacts.setdefault((package_id, name), set()).add(path)
        if not executable:
            return
        target_binding = {key: message.get(key) for key in ('package_id', 'target', 'features', 'profile')}
        target_key = json.dumps(target_binding, sort_keys=True)
        if target_key not in self.expected_targets:
            return  # build scripts/public products never acquire cleanup authority
        if not isinstance(message.get('fresh'), bool):
            raise Denied('compiler artifact must declare a boolean fresh state')
        if message['fresh'] and not self.retain_preexisting:
            raise Denied('only newly produced compiler executables may be reclaimed')
        path = Path(message['executable'])
        if target_key in self.target_paths and self.target_paths[target_key] != path:
            raise Denied('one target produced multiple executable paths')
        if path in self.path_targets and self.path_targets[path] != target_key:
            raise Denied('multiple targets produced one executable path')
        parent, parent_identity, identity, digest = self._open(path, allow_links=True, allow_preexisting=self.retain_preexisting)
        os.close(parent)
        record = (parent_identity, identity, digest, self.binding)
        if path in self.records and self.records[path] != record:
            raise Denied('compiler executable identity changed')
        if path not in self.groups:
            self.groups[path] = self._capture_group(path, identity, digest, reclaimable=not message['fresh'])
        if message['fresh']:
            self.groups[path]['reclaimable'] = False
        self.records[path] = record
        self.target_paths[target_key] = path
        self.path_targets[path] = target_key
        self.captured_targets.add(target_key)

    def _binary_runtime_environment(self, executable, package_id, targets):
        if not isinstance(targets, list) or len(targets) > 256:
            raise Denied('Cargo binary runtime inventory must be a bounded list')
        # Cargo's build.build-dir can differ from CARGO_TARGET_DIR: test
        # harnesses land under the private build root while CARGO_BIN_EXE_*
        # points into the target root. Derive the latter from the admitted
        # target resource, never from the harness executable path.
        profile_dir = self.resources['target'] / 'debug'
        if (not profile_dir.is_dir() or native_path(str(profile_dir)) != profile_dir
                or not any(profile_dir.is_relative_to(root) for root in self.roots)):
            raise Denied('Cargo test executable has no canonical private profile directory')
        resolved = {}
        seen = set()
        for item in targets:
            if not isinstance(item, dict) or not isinstance(item.get('name'), str) or not item['name'] or not isinstance(item.get('enabled'), bool):
                raise Denied('Cargo binary runtime inventory has an invalid row')
            name = item['name']
            key = 'CARGO_BIN_EXE_' + name
            candidate = profile_dir / name
            if ('/' in name or '\\' in name or name in seen or candidate.name != name
                    or native_path(str(candidate)) != candidate):
                raise Denied('Cargo binary runtime inventory has an unsafe or duplicate target name')
            seen.add(name)
            artifacts = self.binary_artifacts.get((package_id, name), set())
            if item['enabled']:
                if artifacts != {candidate} or not candidate.is_file() or candidate.is_symlink():
                    raise Denied('enabled Cargo binary path differs from the exact compiler artifact')
            elif candidate in artifacts:
                raise Denied('feature-disabled Cargo binary unexpectedly produced an executable artifact')
            if os.path.lexists(candidate):
                metadata = candidate.lstat()
                if not stat.S_ISREG(metadata.st_mode) or candidate.is_symlink():
                    raise Denied('Cargo binary runtime path is not a regular non-symlink file')
            resolved[key] = str(candidate)
        return resolved

    def observe(self, producer, env, *, settlement_timeout=30):
        """Supervise setup/metadata/check/public build; observation cannot reclaim."""
        return self._run(producer, {}, env, settlement_timeout=settlement_timeout, observation_only=True)

    def run(self, producer, consumer_args, env, *, settlement_timeout=30):
        if not self.expected_targets:
            self.used = True
            self.close()
            raise Denied('test cohort requires nonempty exact expected targets')
        return self._run(producer, consumer_args, env, settlement_timeout=settlement_timeout, observation_only=False)

    def _run(self, producer, consumer_args, env, *, settlement_timeout, observation_only):
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
            children_capability(self.child_discovery)  # before any resource-bearing process
            tree = ClippyTree(child_discovery=self.child_discovery)
        except BaseException:
            self.close()
            raise
        status = None
        error = None
        consumed = []
        coverage = []
        try:
            child_env = dict(env)
            for key, value in self.descriptor.get('runtime_environment', {}).items():
                if value is None:
                    child_env.pop(key, None)
                else:
                    child_env[key] = value
            expected = {'CARGO_TARGET_DIR': self.resources['target'],
                        'CARGO_BUILD_BUILD_DIR': self.resources['build'],
                        'CARGO_HOME': self.resources['cargo_home'], 'TMPDIR': self.resources['temp']}
            for key, path in expected.items():
                if child_env.get(key) != str(path):
                    raise Denied('producer environment differs from admitted resource: ' + key)
            # A private disk spool preserves diagnostics without pipe deadlocks or
            # accumulating compiler output in memory. It remains retained evidence.
            with self.evidence_path.open('xb+') as output:
                status = self._launch(tree, producer, child_env, stdout=output, journal_phase='producer')
                output.seek(0)
                for line in output:
                    sys.stdout.buffer.write(line)
                    if status == 0 and not observation_only:
                        try:
                            message = json.loads(line)
                        except (ValueError, UnicodeError) as exception:
                            raise Denied('producer stdout is not Cargo JSON') from exception
                        self._record(message)
                output.flush()
                os.fsync(output.fileno())
            if status == 0 and not observation_only:
                if isinstance(consumer_args, dict) and set(consumer_args) == {'by_target'}:
                    by_target = consumer_args['by_target']
                    mapped = {}
                    for path, target_key in self.path_targets.items():
                        target = json.loads(target_key)
                        identity = json.dumps({'package_id':target['package_id'], 'name':target['target']['name'], 'kind':target['target']['kind'], 'src_path':target['target']['src_path']}, sort_keys=True, separators=(',', ':'))
                        if identity not in by_target:
                            raise Denied('captured target lacks exact consumer specification')
                        mapped[str(path)] = by_target[identity]
                    if len(mapped) != len(by_target):
                        raise Denied('target consumer population differs from captured targets')
                    consumer_args = mapped
                if self.captured_targets != self.expected_targets or not self.records or {str(path) for path in self.records} != set(consumer_args):
                    raise Denied('consumer coverage differs from captured executables')
                for path in self.records:
                    current = self._validate_record(path)
                    if current[1:3] != self.records[path][1:3]:
                        raise Denied('executable changed before consumption')
                    specification = consumer_args[str(path)]
                    consumer_env = dict(child_env)
                    cwd = self.worktree
                    arguments = specification
                    binary_environment = {}
                    if isinstance(specification, dict):
                        arguments = specification['args']
                        if 'build_scripts' in specification:
                            package_id = json.loads(self.path_targets[path])['package_id']
                            actual_scripts = [script for script in self.build_scripts if script['package_id'] == package_id]
                            if actual_scripts != specification['build_scripts']:
                                raise Denied('package build-script runtime context differs from checked graph')
                        cwd = native_path(specification['cwd'])
                        if not cwd.is_relative_to(self.worktree) or not cwd.is_dir():
                            raise Denied('consumer cwd must be canonical package directory')
                        for key, value in specification.get('env', {}).items():
                            if key in expected or key in ('TEMP', 'TMP', 'CARGO_ADMITTED_COHORT_DESCRIPTOR'):
                                raise Denied('consumer cannot override admitted resource/owner environment')
                            consumer_env[key] = value
                        if 'cargo_bin_exe' in specification:
                            package_id = json.loads(self.path_targets[path])['package_id']
                            binary_environment = self._binary_runtime_environment(
                                path, package_id, specification['cargo_bin_exe'])
                            consumer_env.update(binary_environment)
                    if isinstance(specification, dict) and specification.get('verify_harness') is True:
                        row = self._consume_harness(tree, path, arguments, consumer_env, cwd)
                        if binary_environment:
                            row['cargo_bin_exe'] = binary_environment
                        status = row['status']
                        coverage.append(row)
                    else:
                        status = self._launch(tree, [str(path), *arguments], consumer_env, cwd=cwd)
                    if status:
                        break
                    consumed.append(str(path))
        except BaseException as exception:
            error = exception
        try:
            settlement_token = None
            if self.journal is not None:
                try: settlement_token = self.journal.begin('settlement')
                except BaseException as failure: error = error or failure
            self.receipt = tree.finish(stop=error is not None, timeout=settlement_timeout)
            if settlement_token is not None:
                try: self.journal.end(settlement_token)
                except BaseException as failure: error = error or failure
            self.finished = True
            if self.journal is not None and self.journal.failure is not None:
                self.receipt['journal_error'] = self.journal.failure
            reclaimed = []
            retained = [str(path) for path, group in self.groups.items() if not group['reclaimable']]
            if not observation_only and error is None and status == 0 and self.receipt['tree_settled'] and not self.receipt['cancelled']:
                # Validate all before deleting any; subsequent per-path validation
                # still fails closed if a cooperative owner contract is violated.
                for path in self.records:
                    if self._validate_record(path) != self.records[path]:
                        raise Denied('executable changed before reclaim')
                with self.receipt_path.open('xb') as evidence:
                    with self._phase('authorization', targets=list(self.path_targets.values())):
                        authorization = {'event': 'authorization', 'product_status': status,
                                         'settlement': self.receipt, 'consumed': consumed, 'coverage': coverage,
                                         'source_profile_binding': self.binding, 'pinned_subjects': {str(path): digest for path, digest in self.pinned_subjects.items()}, 'runtime_environment_sha256': self.descriptor.get('runtime_environment_sha256'),
                                         'alias_groups': [{'executable': str(path), 'file_identity': group['initial_identity'],
                                                           'sha256': group['sha256'], 'reclaimable': group['reclaimable'], 'aliases': [{'path': str(alias['path']), 'parent_identity': alias['parent_identity']} for alias in group['aliases']]} for path, group in self.groups.items()],
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
                    self._receipt_stream = evidence
                    for path in list(self.records):
                        if self.groups[path]['reclaimable']:
                            reclaimed.extend(str(alias) for alias in self._reclaim(path))
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
                    'reclaimed': reclaimed, 'retained': retained, 'binding': self.binding, 'coverage': coverage}
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
        for group in self.groups.values():
            if group['fd'] is not None:
                os.close(group['fd'])
                group['fd'] = None
                for alias in group['aliases']:
                    os.close(alias['parent'])
        self._receipt_stream = None
        if self.ancestor_fd is not None:
            os.close(self.ancestor_fd)
            self.ancestor_fd = None

    def _consume_harness(self, tree, path, arguments, env, cwd):
        if arguments:
            raise Denied('staged coverage supports the original unfiltered default harness arguments only')
        index = self._harness_sequence
        self._harness_sequence += 1
        logs = []
        populations = []
        for suffix, flags in [('all', ['--list', '--format', 'terse']),
                              ('ignored', ['--ignored', '--list', '--format', 'terse'])]:
            log = self.evidence_path.with_name(self.evidence_path.name + '.harness-' + str(index) + '-' + suffix)
            with log.open('xb+') as output:
                status = self._launch(tree, [str(path), *flags], env, cwd=cwd, stdout=output, journal_phase=suffix+'_list')
                output.flush(); os.fsync(output.fileno()); output.seek(0)
                text = output.read().decode('utf-8')
            if status:
                raise Denied('native harness inventory failed')
            names = []
            for line in text.splitlines():
                match = re.fullmatch(r'(.+): (test|benchmark)', line)
                if match is None:
                    raise Denied('unsupported native libtest list output')
                names.append(match.group(1))
            # Rust permits duplicate #[test] attributes: each registration must
            # execute, even when its displayed case name is identical.
            populations.append(Counter(names)); logs.append(str(log))
        all_cases, ignored_cases = populations
        if not ignored_cases <= all_cases:
            raise Denied('native ignored inventory differs from full inventory')
        log = self.evidence_path.with_name(self.evidence_path.name + '.harness-' + str(index) + '-run')
        with log.open('xb+') as output:
            status = self._launch(tree, [str(path)], env, cwd=cwd, stdout=output, journal_phase='harness_run')
            output.flush(); os.fsync(output.fileno()); output.seek(0)
            raw = output.read()
            sys.stdout.write(raw.decode('utf-8'))
        if status == 0:
            text = libtest_text_without_rpc_frames(raw)
            completed = []
            ignored = Counter()
            for line in text.splitlines():
                match = re.fullmatch(r'test (.+?)(?: - should panic)? \.\.\. (ok|ignored(?:, .*)?)', line)
                if match:
                    completed.append(match.group(1))
                    if match.group(2).startswith('ignored'):
                        ignored[match.group(1)] += 1
            if Counter(completed) != all_cases or ignored != ignored_cases:
                actual = Counter(completed)
                differences = {'missing': all_cases - actual, 'excess': actual - all_cases,
                               'ignored_missing': ignored_cases - ignored,
                               'ignored_excess': ignored - ignored_cases}
                detail = {key: {'occurrences': sum(value.values()),
                                'first_names': dict(sorted(value.items())[:16])}
                          for key, value in differences.items()}
                detail.update(executable=str(path), logs=[*logs, str(log)])
                raise Denied('native successful harness run omitted/duplicated cases or changed ignored population: '
                             + json.dumps(detail, sort_keys=True))
        return {'executable': str(path), 'status': status, 'cases': sorted(all_cases.elements()),
                'ignored': sorted(ignored_cases.elements()), 'case_count': sum(all_cases.values()),
                'ignored_count': sum(ignored_cases.values()), 'logs': [*logs, str(log)]}

    def _launch(self, tree, command, env, **kwargs):
        phase = kwargs.pop('journal_phase', 'consumer')
        target = self.path_targets.get(Path(command[0]))
        with self._phase(phase, argv=command, target=target) as token:
            status = self._launch_impl(tree, command, env, **kwargs)
            if token is not None: token['exit_status'] = status
            return status

    def _launch_impl(self, tree, command, env, **kwargs):
        # Known captured executables need both current owner/source validation
        # and a fresh complete alias-group check immediately before Popen.
        # _validate_group performs the owner check itself; do it once.
        group = self.groups.get(Path(command[0]))
        if group is None:
            self._check_owner()
        else:
            self._validate_group(group)
        if env.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children') != self.child_discovery:
            raise Denied('launch discovery profile differs from original admission')
        if getattr(tree, 'child_discovery', None) != self.child_discovery:
            raise Denied('wait owner discovery profile differs from original admission')
        if not callable(getattr(tree, 'reap_adopted', None)):
            raise Denied('cohort requires the qualified active adopted-child reaping service')
        admission = self.descriptor['admission']
        if admission['policy'] in ('byte-budget', 'budget-file'):
            check_capacity(list(self.resources.values()), env, admission)
        elif admission['policy'] == 'percent':
            capacity_env = dict(env, MIN_FREE_GB=str(admission['reserve_bytes'] / 1024 ** 3), MAX_USED_PCT=str(admission['max_used_pct']), CARGO_STORAGE_POLICY='percent')
            check_capacity(list(self.resources.values()), capacity_env)
        else:
            raise Denied('unsupported original admission policy')
        tree.binding = True
        try:
            process = subprocess.Popen(command, env=env, cwd=kwargs.pop('cwd', self.worktree), **kwargs)
            tree.track(process.pid, process)
            if self.journal is not None: self.journal.child(process)
        finally:
            tree.binding = False
        if tree.cancelled:
            raise KeyboardInterrupt('cohort cancelled after child binding')
        return self._wait_active(tree, process)

    @staticmethod
    def _wait_active(tree, process):
        # This worker remains the single-threaded subreaper. Service exited
        # adopted children while an inner consumer still needs actual reaping;
        # Popen retains exclusive leader status ownership. No closure or lease
        # conclusion comes from a service turn: finish() still requires ECHILD.
        while True:
            mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
            try:
                tree.reap_adopted()
                try:
                    return process.wait(timeout=0.01)
                except subprocess.TimeoutExpired:
                    pass
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)

    def _reclaim(self, path):
        with self._phase('reclaim', executable=str(path), target=self.path_targets.get(path)):
            return self._reclaim_impl(path)

    def _reclaim_impl(self, path):
        if not self._cleanup_authorized or self._receipt_stream is None or path not in self.records or not self.finished or self.receipt is None or not self.receipt['tree_settled']:
            raise Denied('cohort has no successful consumption and kernel closure')
        group = self.groups[path]
        if not group['reclaimable']:
            raise Denied('retained compiler executable has no reclamation authority')
        removed = []
        while group['remaining']:
            self._validate_group(group)
            alias = group['remaining'][0]
            os.unlink(alias['path'].name, dir_fd=alias['parent'])
            group['remaining'].pop(0)
            held = os.fstat(group['fd'])
            current = facts(held)
            previous = group['identity']
            if current[:4] != previous[:4] or current[5] != previous[5] or current[-1] != previous[-1] - 1:
                raise Denied('alias inode changed beyond this owned unlink')
            group['identity'] = current  # own unlink legitimately changes ctime/nlink
            self._validate_group(group)
            removed.append(alias['path'])
            # Unlink and receipt append are not transactional. Failure stops the
            # group and preserves remaining aliases; prior removals stay removed.
            # Removing the final name does not free an inode while this
            # captured descriptor remains open. Record the existing fstat
            # observation, not a claim of reclaimed space or a peak bound.
            # No extra scan, persist/fsync, close or lifecycle step is added.
            self._persist(self._receipt_stream, {
                'event': 'reclaimed', 'path': str(alias['path']),
                'held_inode': {'device': held.st_dev, 'inode': held.st_ino,
                               'allocated_bytes': held.st_blocks * 512,
                               'link_count': held.st_nlink, 'descriptor_held': True,
                               'open_unlinked_bytes': held.st_blocks * 512 if held.st_nlink == 0 else 0}})
        del self.records[path]
        return removed


def main(argv=None):
    """Thin CLI bridge; the native producer supplies commands, never settlement."""
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capability-only', action='store_true')
    parser.add_argument('--request', type=Path)
    parser.add_argument('--result', type=Path)
    options = parser.parse_args(argv)
    if options.capability_only:
        print(json.dumps({'children': children_capability(), 'qualification': 'capability only'}))
        return 0
    if options.request is None or options.result is None:
        raise Denied('bridge requires request and exclusive result paths')
    if options.request.stat().st_size > 8 * 1024 * 1024:
        raise Denied('bridge request exceeds bound')
    request_bytes = options.request.read_bytes()
    request = json.loads(request_bytes)
    with options.result.open('x', encoding='utf-8') as output:
        descriptor = json.loads(os.environ['CARGO_ADMITTED_COHORT_DESCRIPTOR'])
        journal = PhaseJournal(options.request, request_bytes, request, descriptor)
        try:
            with journal.phase('construction'):
                instance = Cohort(descriptor, request['source_digest'], request['profile_digest'],
                                  source_subject=request['source_subject'], profile_subject=request['profile_subject'],
                                  expected_targets=request['expected_targets'], evidence_path=request['evidence_path'],
                                  pinned=request.get('pinned', []), pinned_subjects=request.get('pinned_subjects', {}),
                                  retain_preexisting=request.get('retain_preexisting', False), journal=journal)
            if request['operation'] == 'observe':
                result = instance.observe(request['producer'], os.environ.copy())
            elif request['operation'] == 'run':
                result = instance.run(request['producer'], request['consumers'], os.environ.copy())
            else:
                instance.close()
                raise Denied('unknown bridge operation')
        finally:
            journal.close()
        json.dump(result, output, sort_keys=True)
        output.flush()
        os.fsync(output.fileno())
    return result['status']


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (Denied, OSError, ValueError, KeyError) as failure:
        print('cargo-cohort: DENY: ' + str(failure), file=sys.stderr)
        sys.exit(75)
