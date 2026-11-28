# perl-line-index

A tiny microcrate that provides a byte-oriented line index used by incremental parsing paths.

## API

- `LineIndex::new(&str)` builds a line-start cache.
- `LineIndex::byte_to_position(usize)` maps byte offset to `(line, column)`.
- `LineIndex::position_to_byte(line, column)` maps back to byte offsets.
- `LineIndex::position_to_byte_checked(line, column)` returns `None` if the
  column crosses the addressed line's byte boundary.
- `LineIndex::position_to_byte_utf16(text, line, column)` interprets the column
  in UTF-16 code units and requires the same source snapshot used by `new`.

Byte columns and UTF-16 columns are different domains. Byte conversion deliberately
addresses UTF-8 continuation bytes and clamps offsets above EOF. Reverse UTF-16
conversion rejects surrogate-pair interiors and the LF inside CRLF. A final bare CR
is content. No text normalization, forward UTF-16 conversion, range conversion, or
edit-splicing API is supplied by this crate.

## Executable position laws

Issue #19300 owns this bounded verification slice. Production remains in `src/lib.rs`;
no conversion implementation is copied into a model or replaced for verification.

`tests/formal/contract.rs` supplies independent reference relations and shared
assertions. The byte relation counts raw LF bytes before an offset, and reverse
acceptance enumerates addressable bytes instead of reusing the cached line index.
The UTF-16 relation enumerates scalar boundaries, excludes CRLF interiors, and counts
`encode_utf16` units in the independently identified line prefix.

Run the ordinary regression target through the repository's admitted Cargo route:

```bash
scripts/cargo-admitted test -p perl-line-index --lib --locked formal_position_laws
```

The six tests in `tests/formal/position_laws.rs` are included by a `cfg(test)`
module in `src/lib.rs`. The existing `unit_foundation_full` gate runs them through
its unfiltered `cargo test --lib -p perl-line-index` selection; no new workflow or
CI matrix is added. They cover the complete finite text domain, byte clamping and reverse
acceptance, UTF-16 soundness and completeness, literal independent witnesses, an
actual counterexample to the false byte-scalar-boundary theorem, and separate BOM,
NUL, and longer-input regressions. The ordinary tests enumerate short coordinates
and selected machine-word extremes. They do not establish every machine-word
coordinate or a native Kani result.

### Profile and invocation drift controls

Five additional tests in `tests/formal/profile_contract.rs` run as children of the
same library-test module, for eleven ordinary tests in this slice. They bind the
reviewed source layout to the exact positive/negative harness split, per-harness
cover names, unwind bound, target and text-domain assumption, production-source
import, and the two documented native invocations. Mutation controls remove,
duplicate and rename each cover, change scope/entrypoints, merge the negative
control into the positive file, and alter invocation flags or output identities.

These are conservative text/layout checks, not a Rust parser, semantic equivalence
proof, tool execution, or a native result. A legitimate source rewrite can require
an explicit update. Comments or inactive code are not authenticated execution;
#19317 must still reconcile the actually executed harness/assertion/cover sets.
No native result or tool installation is needed for these five maintenance tests.

### Exact native claim

The generator maps case IDs 0 through 584 bijectively onto texts with zero through
three symbols from this alphabet:

```text
a, TAB, CR, LF, é (U+00E9), 中 (U+4E2D), 😀 (U+1F600), combining acute (U+0301)
```

This is `1 + 8 + 64 + 512 = 585` texts, at most 12 UTF-8 bytes, four LF-defined lines,
and six UTF-16 code units. BOM/NUL and longer regression examples do **not** silently
expand this symbolic claim. Invalid UTF-8 is outside an API that takes `&str`.

`tests/formal/line_index_kani.rs` imports the actual `src/lib.rs` and the same
reference predicates. It contains exactly three positive harnesses:

| Harness | Obligation |
| --- | --- |
| `byte_geometry_and_reverse_acceptance` | Correct clamping/geometry, both byte round trips, and independent reverse acceptance/rejection |
| `strict_utf16_soundness` | Every accepted result is an in-bounds scalar boundary with exactly the requested line and UTF-16 column |
| `strict_utf16_completeness` | Every independently representable boundary is accepted, including empty EOF |

Each harness uses an explicit unwind bound of 32. Query coordinates are arbitrary
64-bit `usize` values, with no valid-coordinate assumption. The only domain
assumption is `case < 585`; the generator also asserts UTF-8 validity. Index
construction and queries use the same immutable text snapshot. Soundness alone
would admit an always-rejecting converter; completeness is independently required.

The three harnesses declare **18 required positive cover witnesses**. They cover
empty input, maximum byte/line lengths, CRLF, machine-word extremes, acceptance,
surrogate/CRLF rejection, and EOF for two-/three-/four-byte scalars, combining text,
tab, and an empty trailing line. All 18 must be present and `SATISFIED`; aggregate
verification success is insufficient. Native assertions, safety checks and
unwinding checks must also succeed. The unwind/resource bounds are not yet a
measured performance qualification.

### Native invocation and controls

Use an admitted x86_64 Linux executor with the programme's hash-verified, pinned **Kani 0.68.0**
bundle and its bundled compiler/CBMC. The Kani toolchain is independent of the
repository's ordinary stable toolchain. The following are tool commands for the
common custody runner, not an alternative admission wrapper or a CI activation:

```bash
kani crates/perl-line-index/tests/formal/line_index_kani.rs \
  -Z unstable-options -Z concrete-playback \
  --export-json target/formal/position-positive.json \
  --concrete-playback=print

kani crates/perl-line-index/tests/formal/false_scalar_boundary.rs \
  -Z unstable-options -Z concrete-playback \
  --export-json target/formal/position-false-theorem.json \
  --concrete-playback=print
```

The flags are version-specific. In the pinned Kani commit
`0d2328a93f0e0ff66132d6bfa1a7d884877cf862`,
`kani-driver/src/args/mod.rs` (blob
`fe43c977016efca3b15de081bf26c920ac4a701f`) requires `-Z unstable-options` for
JSON export and `-Z concrete-playback` for replay output.
`kani_metadata/src/unstable.rs` (blob
`9a639d3bc58cdd4a1999fc11758797bd67b57066`) does not define an `export-json`
unstable feature. Do not substitute a command from the moving upstream book for
this pinned CLI contract. Both feature flags above are required; this source audit
does not qualify a native run or its resource budget.

Do not pass `--tests`, disable checks, or use `--concrete-playback=inplace`. The
ordinary `src/lib.rs` unit tests are not part of the native harness denominator.
Create the output directory, invalidate old outputs, impose the admitted process
and resource bounds, retain complete logs and structured results, and wait for
process settlement through #19315/#19317. These raw commands alone do not satisfy
that custody contract.

The second command must **fail the named assertion**
`FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY`: for example, raw byte 1 of `é` is a valid
byte-column result but not a scalar boundary. It is a wrong-theorem control, not a
production bug. A compiler error, timeout, absent harness, or unrelated failed
assertion does not satisfy the control.

Wrong-production controls are separately required before integrated admission.
Apply these only in an isolated materialization and require exactly one matching
source edit, unchanged harness/oracle inputs, a named property failure, retained
mutant identity, and successful restoration:

| Production mutation | Required discrimination |
| --- | --- |
| `let ch_utf16 = ch.len_utf16();` becomes `let ch_utf16 = 1;` | `FV_POSITION_UTF16_SOUNDNESS` or `FV_POSITION_UTF16_COMPLETENESS` fails on a supplementary scalar |
| The final UTF-16 end-of-line return becomes `None` | `FV_POSITION_UTF16_COMPLETENESS` fails, including the empty-text case |
| Remove the `byte.min(self.text_len)` clamp | The forward-byte property or an automatic safety check fails; do not accept a compile error as the expected outcome |

The regression suite is designed to distinguish these changes, but merely listing
mutations is not execution evidence. Their native results must be produced by the
common adapter; this slice does not add a second process runner or result parser.

### Identity, maintenance, and limits

The shared reference file, positive/negative harnesses, and ordinary test module
live under the existing `tests/**` package include rule. The standalone proof is
still repository-source evidence, not packaged-artifact equivalence. Bind its
production source, shared oracle/generator, selected harness, target, flags, unwind
bound, compiler/std model, verifier, solver, and custody/result-parser identity in
the formal receipt. Bind consumer paths separately before claiming a live LSP path.

When a law, alphabet, bound, production API, or cover changes, update the regression
controls and native claim together and invalidate relevant evidence. A smaller
domain or stronger assumption narrows the claim. Preserve the three-harness and
18-cover denominators until an explicitly reviewed claim change replaces them.
Keep the intentionally failing theorem out of every positive invocation.

This source slice does not issue or accept a formal PASS receipt. Native execution,
cover admission, mutation sensitivity, currentness and protected integration remain
separate acceptance obligations under #19313–#19317 and #19319–#19322. A passing
ordinary regression suite is `TESTED`; an accepted native result is `BOUNDED_PROVED`
only for its recorded domain. Neither proves unbounded Unicode correctness,
caller snapshot discipline, live FULL synchronization, range/edit correctness, or
editor behavior.
