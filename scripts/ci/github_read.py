#!/usr/bin/env python3
"""Read bounded GitHub CI observations without gh or additional credentials."""
from __future__ import annotations
import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise ValueError('GitHub redirect refused')


def read(path: str, *, paginate: bool = False, fetch=None):
    repository = os.environ.get('GITHUB_REPOSITORY', '')
    if not re.fullmatch(r'[^/\s]+/[^/\s]+', repository):
        raise ValueError('repository is absent')
    prefix = f'repos/{repository}/'
    if not path.startswith(prefix) or any(c in path for c in ('#', '\\', '\r', '\n')):
        raise ValueError('read must remain in the current repository')
    suffix = path[len(prefix):].split('?', 1)[0]
    if not re.fullmatch(r'(?:git/ref/heads/main|contents/(?:\.github/workflows/ci\.yml|scripts/ci/main_red_refusal\.py)|commits/[0-9a-f]{40}/check-runs|actions/workflows/ci\.yml/runs)', suffix):
        raise ValueError('unsupported observation')
    def request(target):
        token = os.environ.get('GH_TOKEN', '')
        if not token:
            raise ValueError('read token is absent')
        req = urllib.request.Request('https://api.github.com/' + target, headers={
            'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'perl-lsp-ci-read'})
        with urllib.request.build_opener(NoRedirect).open(req, timeout=30) as response:
            raw = response.read(16 * 1024 * 1024 + 1)
            if response.status != 200 or len(raw) > 16 * 1024 * 1024:
                raise ValueError('incomplete observation')
            return json.loads(raw)
    fetch = fetch or request
    if not paginate:
        return fetch(path)
    key = 'check_runs' if suffix.endswith('/check-runs') else 'workflow_runs' if suffix.endswith('/runs') else None
    if key is None:
        raise ValueError('unsupported pagination')
    parts = urllib.parse.urlsplit(path)
    query = dict(urllib.parse.parse_qsl(parts.query, strict_parsing=True))
    if 'page' in query:
        raise ValueError('caller-selected pagination refused')
    query['per_page'] = '100'
    pages, seen, total = [], set(), None
    for page in range(1, 101):
        query['page'] = str(page)
        data = fetch(parts.path + '?' + urllib.parse.urlencode(query))
        count = data.get('total_count')
        rows = data.get(key)
        if type(count) is not int or not 0 <= count <= 10000 or not isinstance(rows, list):
            raise ValueError('invalid page')
        if total is not None and count != total:
            raise ValueError('pagination changed')
        total = count
        for row in rows:
            identifier = row.get('id') if isinstance(row, dict) else None
            if type(identifier) is not int or identifier <= 0 or identifier in seen:
                raise ValueError('duplicate or invalid observation')
            seen.add(identifier)
        pages.append(data)
        if len(seen) == total:
            return pages
        if len(seen) > total or len(rows) != 100:
            raise ValueError('incomplete pagination')
    raise ValueError('pagination bound exceeded')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--paginate', action='store_true')
    parser.add_argument('--slurp', action='store_true')
    parser.add_argument('--field', choices=('.object.sha', '.sha'))
    parser.add_argument('path')
    args = parser.parse_args()
    try:
        if args.paginate != args.slurp or (args.paginate and args.field):
            raise ValueError('unsupported projection')
        data = read(args.path, paginate=args.paginate)
        if args.field:
            for field in args.field.removeprefix('.').split('.'):
                data = data[field]
            if not isinstance(data, str) or not re.fullmatch('[0-9a-f]{40}', data):
                raise ValueError('invalid SHA')
            print(data)
        else:
            print(json.dumps(data))
        return 0
    except (ValueError, KeyError, TypeError, OSError, urllib.error.URLError):
        # Never print response bodies, token values or transport exception details.
        print('GitHub CI read unavailable or incomplete', file=sys.stderr)
        return 1

if __name__ == '__main__':
    raise SystemExit(main())
