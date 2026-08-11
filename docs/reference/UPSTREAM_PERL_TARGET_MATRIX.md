# Upstream Perl target matrix

The compiler harness treats an upstream test target as a versioned selection and invocation contract, not as a display name or a list of local directories.

The pinned Perl 5.42.2 matrix is:

```text
.ci/perl-core-harness/upstream-targets-5.42.2.v1/
```

The directory contains a versioned `index.json` plus sorted target-part files. The validator assembles one canonical typed matrix before fingerprinting it, so file partitioning is review structure rather than denominator identity.

The current `blead` topology observation is:

```text
.ci/perl-core-harness/upstream-targets-blead-drift.v1.json
```

Validate the typed contracts and the drift binding offline with:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-targets -- \
  check \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  .ci/perl-core-harness/upstream-targets-blead-drift.v1.json
```

The Rust command decodes the contracts, checks stable IDs and aliases, validates local and root-external selectors, verifies variant and composite references, rejects cycles, requires an overlap policy for every composite, preserves ordered runner switches, and binds the drift observation to a deterministic SHA-256 matrix fingerprint. It also ratchets the exact required target-ID and topology-source inventory for Perl 5.42.2, so deleting a row cannot remain internally valid. The command does not clone, build, discover, parse, compile, or execute upstream Perl.

Each physical or selector target records two authorities separately. `authority` names the requested entry point, such as a Make target. `selection_authority` names the scheduler or reviewed selector that actually defines membership, such as `t/TEST` or `t/harness`. Environment variants inherit the base selection authority unless they explicitly switch schedulers. This prevents a chain such as `make test -> runtests -> t/TEST` from becoming an opaque prose field.

## Target classes

The matrix keeps these objects distinct:

- **Physical series** own immutable source membership, such as `t/base`, `t/mro`, `test_reonly`, or one MANIFEST population.
- **Selector variants** alter membership through upstream authority, such as the actual `t/TEST --core` target. Its `core_root_lib` population is distinct from the ordinary root-`lib` MANIFEST population.
- **Environment variants** inherit membership while changing source interpretation, terminal policy, ordered switches, typed variant parameters, or environment—for example UTF-16 byte order/BOM variants or no-TTY runs.
- **Generated composites** join independently identified targets under an explicit overlap policy. They do not become a new physical run result. Perl's default `make test` remains a physical invocation; only the repository's historical aggregate views are composites in this first matrix.
- **Preparation-only targets** describe build prerequisites and executable class without creating a compiler denominator.
- **Instrumentation-only targets** add process instrumentation without raising compatibility.

The repository's historical `HarnessProfile::Core` and `HarnessProfile::Full` remain named rows so they cannot be confused with upstream `--core` or Perl's default full test target. Make aliases such as `check`, `test-notty`, and `test-prep` are machine-readable aliases rather than slash-delimited prose.

## Denominator and claim rules

A matrix row establishes target topology and ownership only. It is not parse, compile, semantic, execution, platform, or performance evidence.

A physical target becomes compatibility authority only after its exact membership is frozen into a comparison series, its evidence bundle is complete, and every failure or accepted boundary is typed and governed. Variants reference the underlying target rather than copying or silently changing its denominator. Missing capability, preparation, generated input, native extension, process, or environment state remains separate from product compiler failure.

The pinned row does not move when `blead` changes. The drift receipt records the exact observed `blead` commit and `Makefile.SH`, `t/TEST`, and `t/harness` blob identities. Topology changes are classified against the pinned matrix; source changes alone do not silently mutate its denominator.
