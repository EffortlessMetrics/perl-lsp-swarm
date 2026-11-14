#!/usr/bin/env python3
"""Regression coverage for exact GitHub Actions run-resource identity (#17903)."""
from __future__ import annotations

import unittest
from typing import Any

from scripts.ci.request_qualification import Adapter, Refused, Subject


class FakeAPI:
    def __init__(self, responses: dict[str, dict[str, Any]]):
        self.responses = responses
        self.marker = object()

    def get(self, path: str) -> dict[str, Any]:
        return self.responses[path]

    def get_bytes(self, path: str) -> bytes:
        return path.encode()

    def dispatch(self, path: str, body: dict[str, Any]) -> None:
        self.last_dispatch = (path, body)


def subject() -> Subject:
    return Subject(
        repository='EffortlessMetrics/perl-lsp-swarm',
        repository_id=1244101844,
        pr=1,
        head='1' * 40,
        base='2' * 40,
        control='3' * 40,
        portfolio_sha256='a' * 64,
        policy_sha256='b' * 64,
        app_id=15368,
        check_name='Perl LSP Rust Small Result',
    )


class ExactRunResourceIdentityTests(unittest.TestCase):
    def test_exact_run_resource_rejects_another_run_id(self) -> None:
        path = 'repos/EffortlessMetrics/perl-lsp-swarm/actions/runs/41'
        adapter = Adapter(FakeAPI({path: {'id': 42}}), subject())
        with self.assertRaisesRegex(
            Refused, 'exact run resource returned another run identity'
        ):
            adapter.api.get(path)

    def test_exact_run_resource_accepts_the_requested_run_id(self) -> None:
        path = 'repos/EffortlessMetrics/perl-lsp-swarm/actions/runs/41'
        expected = {'id': 41, 'run_attempt': 2}
        adapter = Adapter(FakeAPI({path: expected}), subject())
        self.assertEqual(adapter.api.get(path), expected)

    def test_run_subresources_are_not_misclassified_as_run_objects(self) -> None:
        path = (
            'repos/EffortlessMetrics/perl-lsp-swarm/'
            'actions/runs/41/artifacts?per_page=100'
        )
        expected = {'total_count': 0, 'artifacts': []}
        adapter = Adapter(FakeAPI({path: expected}), subject())
        self.assertEqual(adapter.api.get(path), expected)

    def test_non_get_transport_capabilities_remain_delegated(self) -> None:
        api = FakeAPI({})
        adapter = Adapter(api, subject())
        self.assertIs(adapter.api.marker, api.marker)
        self.assertEqual(adapter.api.get_bytes('artifact'), b'artifact')
        adapter.api.dispatch('dispatch', {'value': 1})
        self.assertEqual(
            api.last_dispatch, ('dispatch', {'value': 1})
        )


if __name__ == '__main__':
    unittest.main()
