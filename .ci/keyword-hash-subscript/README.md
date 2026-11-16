# Keyword-shaped hash-subscript oracle: bounded slice

This is a partial, independently authored #14059 matrix consumed through the public
`perl_parser_core::Parser`. It changes no production parser behavior and does not
close #14059 or controller #14056.

The selected denominator is `next`, `last`, `redo`, `return` × direct scalar,
arrow scalar, direct hash slice × terminal and immediate comma: **24 rows**.
Twenty controls retain ordinary/quoted keys, strict rejection, labeled control,
ordinary return, and operand-bearing return. The latter is essential:
`return, $x` has an empty return and a sibling; `return $x, 7` owns both operands.

## Authority and profile

Sources and native expectations were authored from independent real-Perl compile
and complete `B::Deparse,-p` output, without consulting the production keyword
whitelist or copying native output. `cases.json` retains exact UTF-8 source bytes,
source hashes, form/context, subject range, expected AST variants/payloads/child
order/ranges, forbidden alternatives, and mismatch owners.

The sole proven profile is Perl **5.40.1**,
`x86_64-linux-gnu-thread-multi`, `use strict; use warnings;`, no enabled optional
features, and an empty inherited Perl option/library environment. Other versions,
architectures, and feature profiles are `NOT_PROVEN`. The runner uses the installed
Perl identity and process-bound compile/Deparse observations, following the existing
`scripts/ci/signature_conformance.py` instrument pattern. It never executes fixture
bodies, fetches Perl, or regenerates expectations. Invocation errors, timeout,
output limit, unavailable/wrong profile, or signal are not agreement. Rejected
code's partial Deparse stdout cannot hide its nonzero compile/Deparse status.

AST representation follows the existing public contract: `HashSlice { target, keys }`
for direct slices (`perl-ast/src/ast.rs`), scalar accesses as `Binary`, and quoted
`String.value` retains source delimiters (`literal_structural_tests.rs`). The first
native run exposed incorrect harness mappings for statement wrappers, slices, and
quoted payloads; those API mappings were corrected, preserving every Perl source,
Deparse expectation, key ownership, child order, and authored range. This does not
retarget language expectations to a native mismatch.

## Commands

From the repository root:

```bash
python3 scripts/ci/keyword_hash_subscript_oracle.py --output /absolute/path/perl-receipt.json
python3 -m unittest discover -s scripts/ci -p test_keyword_hash_subscript_oracle.py -v
cargo fmt --all -- --check
cargo test -p perl-parser-core --test keyword_hash_subscript_oracle_matrix --locked
KEYWORD_SUBSCRIPT_NATIVE_REPORT=/absolute/path/native.json cargo test -p perl-parser-core --test keyword_hash_subscript_oracle_matrix --locked native_keyword_hash_subscript_conformance_report -- --ignored --exact
cargo clippy -p perl-parser-core --test keyword_hash_subscript_oracle_matrix --locked -- -D warnings
git diff --check
```

Use `scripts/cargo-admitted` and the host's exact-command budget for Rust work under
`docs/agents/CARGO_STORAGE.md`. The staged Clippy route requires `--profile agent`;
record that profile separately. No global capacity threshold or profile is lowered.

Default native tests assert all **38** currently conforming rows, seal the six known
exclusions, and challenge false static controls, child order, direct/arrow/slice
identity, equal-substring ranges, missing evidence, and terminal-as-comma reuse.
Python controls seal all 44 ID→source identities, duplicates, stale bytes, profiles,
owner exclusions, unavailable instruments, and partial rejection. These are native
contract/instrument tests; they do not themselves claim a fresh real-Perl run.

The explicit native conformance report visits **all 44** rows and intentionally
fails for current #14065 discrepancies. Known mismatches remain visible; the report
must never bless them because they have an owner. Generic native recovery cannot
prove the intended strict-subs rejection and remains `NOT_PROVEN` for that comparison.
The retained Perl receipt is external oracle evidence; the native report is a
separate observation, never the source of expectations.

## Remaining denominator

`denominator.remaining` is deliberately nonempty. Remaining work includes
`goto continue undef eval not and or xor do local cmp tie untie try defer`, the
remaining declaration, condition/loop, package/module, OOP, phase, exception and
recovery-sensitive canonical keyword families; `q qq qw qx qr m s tr y` through
existing quote-like lexical authority; dereference slices, fat arrows/hash literals,
Unicode and standalone numeric/dynamic keys, nested operators/calls, trailing commas,
comments/newlines, malformed/missing closers, other control operands, and other
versions/features. Those are exclusions, not implicit dispositions or an all-keyword
completeness claim. New canonical families are not covered merely by these counts.

## Retained observations

`perl-5.40.1.json` is the executed 44-row external receipt bound by matrix, runner,
source and executable SHA256 hashes. `native-d21c4486cb.json` is the separate
all-row observation against production base
`d21c4486cb66f722b983b3b9f70f5e638fe0b3d3`: 38 native matches and six deliberate
`CONFORMANCE_MISMATCH` rows. The three strict ordinary-comma controls compile-reject
but parse cleanly; the three operand-bearing returns lose list ownership. All six
retain #14065 ownership. They are exposed rather than rewritten to current output.
