#!/usr/bin/env python3
"""Observe an already admitted command; never grants admission or cleanup authority."""
import argparse
import json
import math
import os
from pathlib import Path
import signal
import stat
import subprocess
import threading
import time


class MeasurementError(RuntimeError):
    pass


def allocation(roots, stop=None, diagnostics=None):
    """GNU du semantics: unique (device, inode) allocated blocks, no followed links.

    Vanishing entries are a reported concurrent-scan limitation, not an unreadable
    tree. Other I/O errors remain fatal. This is never an atomic filesystem snapshot.
    """
    seen = set()
    total = 0
    races = 0
    pending = list(roots)
    while pending:
        if stop is not None and stop.is_set():
            raise MeasurementError('allocation sample interrupted')
        path = pending.pop()
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            races += 1
            continue
        identity = (metadata.st_dev, metadata.st_ino)
        if identity in seen:
            continue
        seen.add(identity)
        total += metadata.st_blocks * 512
        if stat.S_ISDIR(metadata.st_mode):
            try:
                with os.scandir(path) as entries:
                    pending.extend(Path(entry.path) for entry in entries)
            except FileNotFoundError:
                races += 1
    if diagnostics is not None:
        diagnostics['vanished_entries'] = races
    return total


def observe(roots, stop=None):
    started = time.monotonic()
    filesystems = {}
    for root in roots:
        ancestor = root
        while not ancestor.exists():
            if ancestor.parent == ancestor:
                raise MeasurementError(f'no existing ancestor: {root}')
            ancestor = ancestor.parent
        identity = ancestor.stat().st_dev
        stat = os.statvfs(ancestor)
        filesystems.setdefault(str(identity), {'observed_at': str(ancestor),
            'free_bytes': stat.f_bavail * stat.f_frsize,
            'total_bytes': stat.f_blocks * stat.f_frsize})
    diagnostics = {}
    allocated = allocation(roots, stop, diagnostics)
    return {'allocated_bytes': allocated, 'filesystems': filesystems,
            'scan_seconds': time.monotonic() - started, **diagnostics}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', action='append', required=True, type=Path)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--interval', type=float, default=1.0)
    parser.add_argument('--min-free-bytes', type=int, default=0,
                        help='Cancel the admitted owner if a sample falls below this reserve')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or not math.isfinite(args.interval) or args.interval <= 0 or args.min_free_bytes < 0:
        parser.error('a command and positive finite interval are required')
    roots = [root.absolute() for root in args.root]
    # Existing evidence must never be truncated. Durable evidence stays outside observed roots.
    evidence = args.evidence.absolute()
    if any(evidence == root or root in evidence.parents for root in roots):
        parser.error('evidence must be outside observed roots')
    stop = threading.Event()
    samples = []
    cancelled = []
    started = time.monotonic()
    with evidence.open('x', encoding='utf-8') as stream:
        def write(row):
            stream.write(json.dumps(row, sort_keys=True) + '\n')
            stream.flush()
            os.fsync(stream.fileno())

        def sample(kind, event=None):
            row = {'kind': kind, 'elapsed_seconds': time.monotonic() - started}
            try:
                row.update(observe(roots, event))
                row['valid'] = True
            except (OSError, MeasurementError) as error:
                row.update(valid=False, error=str(error))
            samples.append(row)
            write(row)

        write({'schema': 'cargo-storage-measurement.v1', 'kind': 'header',
               'roots': [str(root) for root in roots], 'interval_seconds': args.interval,
               'min_free_bytes': args.min_free_bytes,
               'limitations': 'Sampled allocation is a lower bound on transient peak; scans are non-atomic and vanishing entries are counted. Free-space observations include unrelated writers. Missing roots count as zero only at observation time. No admission, quota, settlement or cleanup authority.'})
        sample('initial')
        def reserve_lost(row):
            return row['valid'] and any(fs['free_bytes'] < args.min_free_bytes
                                       for fs in row['filesystems'].values())
        if not samples[-1]['valid'] or reserve_lost(samples[-1]):
            return 75
        child = subprocess.Popen(command)
        def signal_child(signum):
            if child.poll() is None:
                try:
                    child.send_signal(signum)
                except ProcessLookupError:
                    pass
        def interrupt(signum, _frame):
            cancelled.append(signum)
            signal_child(signum)
        old = {sig: signal.signal(sig, interrupt) for sig in (signal.SIGINT, signal.SIGTERM)}
        def monitor():
            while not stop.wait(args.interval):
                try:
                    sample('periodic', stop)
                except OSError:
                    # Durable evidence failure must fail the measurement and stop work.
                    samples.append({'valid': False, 'error': 'measurement persistence failed'})
                    signal_child(signal.SIGINT)
                    return
                if reserve_lost(samples[-1]):
                    signal_child(signal.SIGINT)
                    return
        worker = threading.Thread(target=monitor)
        worker.start()
        try:
            status = child.wait()
        finally:
            stop.set()
            worker.join()
            for sig, handler in old.items():
                signal.signal(sig, handler)
        sample('retained')
        valid = [row for row in samples if row['valid']]
        complete = all(row['valid'] for row in samples)
        reserve_preserved = all(not reserve_lost(row) for row in samples)
        write({'kind': 'summary', 'command_status': status, 'cancelled_signals': cancelled,
               'elapsed_seconds': time.monotonic() - started, 'sample_count': len(samples),
               'measurement_complete': complete,
               'observed_reserve_preserved': reserve_preserved,
               'sampled_peak_allocated_bytes': max(row['allocated_bytes'] for row in valid),
               'retained_allocated_bytes': samples[-1].get('allocated_bytes'),
               'peak_is_bound': False})
    if cancelled:
        return 130
    if not complete or not reserve_preserved:
        return 75
    return status if status >= 0 else 128 - status


if __name__ == '__main__':
    raise SystemExit(main())
