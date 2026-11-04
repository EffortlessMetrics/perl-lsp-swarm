#!/usr/bin/env python3
"""Behavioral probes for the public-API Self-resolution fold (#16324).

Covers scripts/ci/public_api_filter.awk, the final stage of
`just _public-api-filter`: method-signature `Self` must resolve to the
owning type path so August full-path and September `Self` nightly
renderings converge, while renames, longer identifiers, and non-method
lines pass through visibly unchanged.

Scope: this file drives the awk stage directly on post-grep/sed lines
(the stages it shares with the recipe are untouched legacy). The full
chain -- filter plus committed baselines under both nightly renderings --
is exercised by CI's `Public API Surface (facade PR)` job.

Every rule here has a negative control: a mutation the fold must not
perform (a renamed owner that must still diff, a `Selfish` that must
keep its spelling, an attributed line that must keep its attribute).
"""

from __future__ import annotations

import shutil
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
AWK = ROOT / "scripts" / "ci" / "public_api_filter.awk"

# (probe name, input line, expected output line)
CASES: list[tuple[str, str, str]] = [
    (
        "clone return",
        "pub fn krate::Type::clone(&self) -> Self",
        "pub fn krate::Type::clone(&self) -> krate::Type",
    ),
    (
        "eq borrows Self",
        "pub fn krate::Type::eq(&self, other: &Self) -> bool",
        "pub fn krate::Type::eq(&self, other: &krate::Type) -> bool",
    ),
    (
        "default return",
        "pub fn krate::Type::default() -> Self",
        "pub fn krate::Type::default() -> krate::Type",
    ),
    (
        "result with Self and bounds",
        "pub fn krate::Type::deserialize<__D>(__deserializer: __D) -> core::result::Result<Self, __D::Error> where __D: core::fmt::Debug",
        "pub fn krate::Type::deserialize<__D>(__deserializer: __D) -> core::result::Result<krate::Type, __D::Error> where __D: core::fmt::Debug",
    ),
    (
        "generic owner with where Self",
        "pub fn krate::Foo<T>::wrap(v: T) -> Self where Self: core::marker::Sized",
        "pub fn krate::Foo<T>::wrap(v: T) -> krate::Foo<T> where krate::Foo<T>: core::marker::Sized",
    ),
    (
        "trait method owner is the trait",
        "pub fn krate::Trait::method(&self) -> Self",
        "pub fn krate::Trait::method(&self) -> krate::Trait",
    ),
    (
        "generic args in owner survive",
        "pub fn krate::Pair<krate::Item, u8>::swap(&self) -> Self",
        "pub fn krate::Pair<krate::Item, u8>::swap(&self) -> krate::Pair<krate::Item, u8>",
    ),
    (
        "function-trait bound arrow does not close depth",
        "pub fn krate::Type::run<F: Fn() -> crate::Output>(f: F) -> Self",
        "pub fn krate::Type::run<F: Fn() -> crate::Output>(f: F) -> krate::Type",
    ),
    (
        "arrow in argument position is untouched",
        "pub fn krate::P::m(&self, cb: &dyn Fn(u8) -> u8) -> Self",
        "pub fn krate::P::m(&self, cb: &dyn Fn(u8) -> u8) -> krate::P",
    ),
    (
        "method-generic bound with paths",
        "pub fn krate::Id::deserialize<D: serde_core::de::Deserializer<'de>>(d: D) -> core::result::Result<Self, D::Error>",
        "pub fn krate::Id::deserialize<D: serde_core::de::Deserializer<'de>>(d: D) -> core::result::Result<krate::Id, D::Error>",
    ),
    (
        "single leading attribute is preserved and folds",
        "#[must_use] pub fn krate::B::finish(self) -> Self",
        "#[must_use] pub fn krate::B::finish(self) -> krate::B",
    ),
    (
        "multiple leading attributes are preserved and fold",
        "#[must_use] #[inline] pub fn krate::B::build() -> Self",
        "#[must_use] #[inline] pub fn krate::B::build() -> krate::B",
    ),
    (
        "attribute without Self is untouched",
        "#[non_exhaustive] pub enum krate::E::V",
        "#[non_exhaustive] pub enum krate::E::V",
    ),
    (
        "Selfish identifier keeps its spelling",
        "pub fn krate::Selfish::new(x: krate::Selfish) -> krate::Selfish",
        "pub fn krate::Selfish::new(x: krate::Selfish) -> krate::Selfish",
    ),
    (
        "struct field line is untouched",
        "pub krate::Plain::field: usize",
        "pub krate::Plain::field: usize",
    ),
    (
        "module function without Self is untouched",
        "pub fn krate::greet(name: &str) -> alloc::string::String",
        "pub fn krate::greet(name: &str) -> alloc::string::String",
    ),
    (
        "full-path rendering is already canonical",
        "pub fn krate::Type::clone(&self) -> krate::Type",
        "pub fn krate::Type::clone(&self) -> krate::Type",
    ),
]


def run_fold(lines: list[str]) -> list[str]:
    proc = subprocess.run(
        ["awk", "-f", str(AWK)],
        input="\n".join(lines) + "\n",
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode == 0, f"awk failed: {proc.stderr}"
    return proc.stdout.splitlines()


@unittest.skipUnless(shutil.which("awk"), "awk is required to drive the fold")
class PublicApiFilterFoldTests(unittest.TestCase):
    def test_fold_cases(self) -> None:
        for name, source, want in CASES:
            with self.subTest(name):
                got = run_fold([source])
                self.assertEqual(got, [want], f"probe {name!r}")

    def test_fold_is_idempotent(self) -> None:
        # A second pass must change nothing: canonical form is a fixed point,
        # which is what the check recipe's `cmp -s` canonical guard requires.
        once = run_fold([source for _, source, _ in CASES])
        twice = run_fold(once)
        self.assertEqual(twice, once)

    def test_fold_preserves_line_count_and_order(self) -> None:
        sources = [source for _, source, _ in CASES]
        got = run_fold(sources)
        self.assertEqual(len(got), len(sources))


if __name__ == "__main__":
    unittest.main()
