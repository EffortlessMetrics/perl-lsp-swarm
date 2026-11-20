#!/usr/bin/env python3
"""Restore a private NUL-delimited environment inside the bounded scope."""
import os
import re
import sys


def main():
    if len(sys.argv) != 3 or sys.argv[2] not in ('primary', 'fallback'):
        raise ValueError('invalid guidance invocation')
    path, mode = sys.argv[1:]
    with open(path, 'rb') as source:
        data = source.read()
    os.unlink(path)
    if not data.endswith(b'\0'):
        raise ValueError('incomplete environment snapshot')
    env = {}
    for pair in data[:-1].split(b'\0'):
        name, sep, value = pair.partition(b'=')
        if not sep:
            raise ValueError('invalid environment snapshot')
        if re.fullmatch(rb'[A-Za-z_][A-Za-z0-9_]*', name):
            env[name] = value
    command = [b'cargo', b'xtask', b'ripr-review-comments',
               b'--base', env[b'RIPR_BASE_SHA'], b'--head', env[b'RIPR_EVALUATED_SHA'],
               b'--pr-head', env[b'PR_HEAD_SHA'], b'--timeout-seconds', b'3600']
    if mode == 'primary':
        command = [b'bash', b'scripts/ci/observe-xtask-build.sh', b'host-guidance',
                   env[b'RIPR_FRESHNESS_HANDOFF'] + b'/xtask-builds', b'--'] + command
    os.execvpe(command[0], command, env)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError):
        print('could not restore hosted guidance environment', file=sys.stderr)
        sys.exit(1)
