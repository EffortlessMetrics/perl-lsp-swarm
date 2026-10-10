# Native Cargo-run loader boundary

Consumer d1ae5de34133509a28f9f7f8aa2b9052b0fb4039 reached the nested owner
after its xtask bootstrap but refused Cargo-generated `LD_LIBRARY_PATH` before
perllsp preparation. Initial ambient loader rejection remains unchanged.

Only the exact outer bootstrap `run -p xtask --bin xtask --locked [--offline]`
binds a runtime loader contract, before budget scope and launch. Program arguments
after `--` do not select the Cargo profile. The protocol names three ordered
absolute paths computed from the bound resource/toolchain facts:

1. private final target root / `debug`;
2. private intermediate build root / `debug/deps`;
3. pinned toolchain / `lib/rustlib/HOST/lib`.

The descriptor/snapshot, directed owner ancestry, original lease and marker,
installed toolchain and bound native roots are checked before the runtime list.
The list must match byte-for-byte; empty, relative, duplicate, reordered, linked,
foreign-profile, extra build-script search paths and arbitrary subtrees refuse.
Native Linux Cargo-run execs xtask, so there is no live Cargo ancestor. The
running exact xtask executable must be a direct child of the original owner;
its kernel executable device/inode must match the current native private path.
The original owner process fact, including start identity, must still match.

Only after this proof can the validation environment omit `LD_LIBRARY_PATH` for
resource-discovery/source Git. Full canonical resource, committed source,
renderer, Cargo configuration and frozen compiler/network/build selectors still
must validate before the normalized environment is returned to compiler children.
Other loader selectors remain refused. This is a compatibility seam under the
existing owner, not a sandbox or a source-to-binary artifact-handoff proof.

## Evidence

Local installed Cargo 1.95.0 probes compiled two tiny dependency-free diagnostic
fixtures (0.15 seconds each), with separate private target/build/cache roots;
no repository product was built. The first observed the three paths. The second
falsified the proposed live-Cargo-ancestor requirement and then passed actual
Cargo -> exec xtask -> Python validation with normalization. Warm repeat was
0.00 seconds. Preserve the failed and successful probes in the local archive.

The consumer owner independently reported a clean retained-bootstrap capture:
1.256 seconds, 369 fresh compiler-artifact records, 299 Fresh lines, no Compiling
lines, the same three ordered paths, one job/incremental off, ordinary debug
profile, positive kernel ECHILD and lease absence. This corroborates the current
Linux Cargo-run/debug shape; it does not reconstruct the historical failed
invocation's loader value or qualify Cargo-test composition.

34 focused tests and 144 combined tests passed (143 passed, one opt-in product
build skipped). Six mutants failed by semantic assertions with zero instrument
errors: bypass validation, retain compiler loader, omit runtime ancestry, accept
arbitrary loader list, leak loader into resource-discovery Git, omit direct owner.
The original implementation produced the intended red assertion for legitimate
Cargo runtime input. Proof archive: `/workspace/clippy-publication-proof/loader-repair`.

## Integration and limits

This child-only delta is based on stable af6807f79a2b98df1305724cc48c23db66943db9.
It does not duplicate the consumer's finite-row additions, binary handoff or the
storage marker repair. Integrate those owners separately and rerun the cheap
combined checks. `nested_command` moves its existing original-marker check before
resource discovery; keep that check against the creation-time marker identity.

Changed owner source and plan binding invalidate old request budgets and snapshots:
regenerate preflight scope rather than reuse the historical budget. No new lease
or recursive admission is introduced. Default 40 GiB reserve is unchanged.

Consumer migration execution, source/artifact handoff, aggregate growth/RAM/cache,
full workload inventory and runtime qualification remain separate obligations.
Cargo-test/build-script loader composition, other profiles/targets/platforms and
hosted activation remain unsupported here. No product build, PR, GitHub comment,
manual CI dispatch or merge is part of this repair.
