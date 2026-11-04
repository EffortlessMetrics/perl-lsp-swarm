# Signature conformance evidence (#8912)

`cases.json` is the independently authored denominator. It records exact source,
version-conditioned compile acceptance, warning categories, selected runtime output,
and expected native header/parameter/variable/default/operator byte geometry. It is
not generated from parser output. The authority links are pinned to Perl 5.44.

Run the instrument tests:

```text
python scripts/ci/test_signature_conformance.py
cargo test --locked -p perl-parser-core --test signature_conformance
```

A green ordinary Rust test means the matrix/comparator instrument works. It does
**not** mean native signature conformance passes: the conformance test is explicitly
opt-in because this evidence-only change exposes unfixed production defects.

Run and retain actual interpreter evidence, once per required version:

```text
python scripts/ci/signature_conformance.py --perl /path/to/perl --expected-version 5.44 --output /path/to/perl-5.44.json
python scripts/ci/signature_conformance.py --validate-receipts /path/to/perl-5.36.json /path/to/perl-5.38.json /path/to/perl-5.42.json /path/to/perl-5.44.json
```

The runner records the actual executable digest/version, source and matrix digests,
exit/stdout/stderr, timeout/instrument status, compile disposition, warnings, and
runtime observations separately. Receipt validation rechecks observations against
current expectations; stored `failures: []` is never authority. Historical 5.32/5.34
runs do not replace any required version. Missing, stale, or wrong-version evidence
is `NOT_PROVEN`. Receipts must come from trusted CI execution or an inspected local
run; hashes bind subject content but do not authenticate arbitrary JSON. Warning identities are matched from pinned diagnostic patterns,
not inferred merely from nonempty stderr.

Run native proof with a retained report (PowerShell):

```powershell
$env:SIGNATURE_NATIVE_REPORT = 'target/signature-conformance/native.json'
cargo test --locked -p perl-parser-core --test signature_conformance native_signature_conformance_report -- --ignored --exact --nocapture
python scripts/ci/signature_conformance.py --check-native-report $env:SIGNATURE_NATIVE_REPORT --native-exit-code $LASTEXITCODE
```

Exit 1 from the Python native validator means a complete report records real
`CONFORMANCE_MISMATCH`. Exit 2 means missing/stale evidence or an instrument/process
failure. The reporter reads default operator text and spans from the AST; it compares
missing or inconsistent operator metadata as a mismatch and never manufactures
a span by examining the source gap. Tree-sitter and Pest are
not observed by this native reporter and are not claimed to conform.

The existing **Perl Version Matrix** workflow has a `signatures_only` dispatch
input. It provisions only 5.36, 5.38, 5.42, and 5.44 and runs one native Rust job;
it skips unrelated postfix and LSP smoke work in that mode. Default runs retain
existing behavior and add the 5.44 interpreter plus signature probes on the four
required versions. Oracle and native artifacts are uploaded even on failure.
Native mismatches deliberately fail their dedicated job, rather than being hidden
by `continue-on-error`. These new jobs are advisory under current main protection,
not new required merge gates. A green required gate does not establish signature
conformance; promotion requires the four-version receipts and subsequent production
repairs to satisfy the native contract.

Known contrary tests include:

- `fix_signature_param_validation_3359::test_sub_signature_invocant_separator_is_accepted`
  accepts `$self: $x`, which is not Perl subroutine-signature syntax.
- `fix_signature_param_validation_3359::test_empty_signature_no_diagnostics`
  checks diagnostics without distinguishing an exact-zero-arity signature from a
  prototype under effective feature context.
- `fix_mixed_positional_named_signature_3944::mixed_positional_and_named_signature_parses_with_expected_shape`
  and `fix_mixed_positional_named_signature_3944::bare_named_parameter_without_default_is_required`
  lack interpreter-version and warning observations; these are not external conformance proof.

The generated native report identifies current mismatches by case and field.
Production repair remains #8915 (parameter forms/defaults), #8917 (ordering), and
#8922 (effective feature admission). No parser, semantic, provider, or editor support
claim is changed by this evidence surface. Revert its fixtures, runner, tests, and
workflow wiring together to roll back the surface; no production state is migrated.

Native dispositions use the canonical recovery salvage classification: advisory
diagnostics do not reject clean syntax, while error nodes do. Any typed terminal
stop is retained as `NOT_PROVEN`, never evidence that a negative fixture was
correctly rejected. Named binding/requiredness and absent defaults are compared
explicitly. Ordinary controls mutate actual parse outputs and named AST fields.

Oracle receipt schema `signature-oracle-receipt/v2` hashes matrix and runner
repository text after replacing CRLF with LF. All other bytes remain significant.
This makes Git line-ending conversion portable without ignoring content changes.
Executable SHA-256 remains byte-exact; fixture source hashes also remain exact.
Version-1 receipts are not silently reinterpreted: retain their original evidence,
then rerun the bounded oracle to produce current version-2 receipts.

The #8915 native parameter-form repair adds required `default_operator` and
`default_operator_span` fields to public `NodeKind::OptionalParameter`, plus
`default_operator_span` to `NamedParameter`. Explicit Rust constructors and
exhaustive field patterns must migrate. Named operator/span/default presence
remains paired; mandatory named parameters carry none. Parser-produced spans are
inside the parameter and come from the consumed operator token.

Invalid aggregate forms and immediately missing defaults are blocking native
`InvalidSignatureParameter` diagnostics with a typed kind and half-open range.
Their Error nodes retain available variable/default evidence. Complete forbidden
defaults and immediately missing expressions preserve signature separators and
suffixes; arbitrary malformed expressions retain existing bounded recovery.
Native range preservation includes clone and inline-reparse offsets. This does
not promote LSP highlighting or semantic/provider behavior. The ordering extension is described below; effective feature admission and
separator/dialect admission remain #8922 and #8925 respectively.

The #8917 ordering extension adds same-sigil duplicate slurpies, optional
positional-after-named, and optional-named-before-required-named cases. The last
case is accepted starting with Perl 5.44 and retains the named-parameter warning;
older rejection of named syntax does not prove its ordering semantics. Adding
rows changes the matrix digest, so earlier 27-row receipts cannot establish the
31-row denominator.

The native ordering owner is `validate_signature_ordering`. It emits
`ParseError::InvalidSignatureOrdering` with `InvalidSignatureOrderingKind` and the
complete offending parameter range, retaining the signature rather than
reordering it. Error parameters add no inferred ordering state. The new public
error variant is additive to the existing `#[non_exhaustive] ParseError`; it does
not newly break external exhaustive matches. The diagnostic kind and message
change for ordering failures. Existing location/anchor consumers receive the range
start; this does not promote full-range LSP diagnostics or call-site binding.
