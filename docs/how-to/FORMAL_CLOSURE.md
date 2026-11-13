# Formal input identity and freshness

Owner: #19314, under #19297 / #19296. Receipt vocabulary: #19313 / PR #19323.

This component computes identity for an **explicitly declared, reviewed input
boundary** and compares a historical formal receipt with that boundary. It does
not launch a verifier, authenticate tool bytes or artifacts, discover arbitrary
Rust dependencies, or establish live-consumer correspondence.

Both commands emit `proof_accepted: false` and
`closure_completeness: not_established`. A `current` result means the declared
semantic inputs are equal, not that a native theorem passed or that the declaration
is complete. A malicious or incomplete declaration is not made trustworthy by
hashing it. The native owner must establish the complete applicable boundary before
using this comparison in proof admission.

## Commands implemented by this candidate

Use the repository-pinned toolchain and existing execution admission. The dedicated
binary is the documented exception to ordinary `cargo xtask` task invocation:

```bash
cargo run -p xtask --bin formal-closure --locked -- \
  --repo-root . compute --spec path/to/committed-spec.json

cargo run -p xtask --bin formal-closure --locked -- \
  --repo-root . compare --spec path/to/committed-spec.json \
  --receipt target/formal/prior-receipt.json

cargo test -p xtask --bin formal-closure --locked formal_closure::tests -- \
  --test-threads=1 --color never
```

These commands are available on the candidate containing this module, not asserted
to be on main before integration. The test filter selects the closure suite only;
the imported receipt module has its own tests. There are 33 closure tests on Unix
and 31 on other targets because native symlink/mode controls are Unix-specific.
Those counts describe the source suite, not an execution result.

`--repo-root` precedes the subcommand and must identify the exact repository root.
The spec is a literal repository-relative path read from its committed blob. It is
not loaded from an arbitrary external path. The receipt is an explicitly selected,
bounded regular JSON file: it is untrusted historical data, not an authenticated
execution record. Only stdout carries output; neither command writes a receipt,
changes the Git index/branch, or executes a command from the spec.

Exit 0 means computation succeeded without declared unresolved inputs, or the
comparison found equal declared inputs. It never means formal PASS. Exit 1 reports
stale, retired or superseded evidence. Exit 2 reports unknown, malformed,
inconsistent, unsupported or unavailable inputs. `--help` also exits 0. Preserve
stderr and the actual exit code; a shell pipeline must not turn refusal into pass.
Unavailable current inputs also produce a bounded diagnostic; raw Git stderr is
not copied into that diagnostic.

## Specification contract

`formal_closure_spec.v1` is a closed typed document. Unknown fields, duplicate JSON
keys at every depth, malformed identities and floating-point numbers are refused.
No `complete: true`, timestamp, native-result or proof-acceptance field is accepted.

| Field | Meaning |
| --- | --- |
| `schema_version` | Exactly `formal_closure_spec.v1` |
| `source_policy` | Exactly `clean_only` in this version |
| `claim` | Existing receipt claim identity: stable ID, generation, owner issue, profile, method, binding and rollout |
| `members` | Unique canonical paths, each with a nonempty set of semantic roles |
| `exclusions` | A reason for each role absent from the declared boundary; not a completeness certification |
| `toolchain` | Existing receipt tool components, versions, SHA-256 digests, target, features and cfg |
| `bounds` | Existing receipt typed bounds, assumptions, fairness, wall limit and output limit |
| `command` | Ordered argument-vector identity; never executed by this component |
| `dependency_model` | Named SHA-256 identities for the applicable external dependency/compiler-library model |
| `environment` | Declared semantic environment values, not the host's entire inherited environment |
| `unknown_inputs` | Explicit unresolved dependencies; any entry prevents `current` |

The role vocabulary is closed:

```text
kernel, wrapper, consumer, enforcement_boundary, manifest, lockfile,
build_input, generated_source, generator_input, model, harness, property,
assumption, control, symbolic_domain, correspondence
```

Every role must occur on a member or have one nonblank exclusion reason, never
both. Harness, property and control sources are mandatory. Non-model bindings also
require a production kernel. A live-consumer declaration requires wrapper,
consumer, final enforcement-boundary and correspondence members. Generated source
requires generator inputs. These checks prevent obvious omissions; they cannot
establish that every relevant member of an included role was named.

Multiple roles may describe one file. Member paths are literal, unique, root-relative
ASCII paths; parent/current-directory components, empty components, backslashes,
pathspec magic, absolute paths, Git metadata and archived paths are refused.
Missing files, submodules and committed symlinks are not followed. Unicode or other
unsupported path spellings fail closed in this initial profile rather than being
silently normalized.

Do not fill tool or dependency digests with arbitrary placeholders and interpret a
computed identity as qualification. This component validates digest shape and binds
the supplied values. #19315 must independently verify the executable, compiler,
sysroot and solver bytes. #19317 must derive the native result from actual execution.
An exclusion reason such as “not applicable” requires review against the real
production/compiler path; it is not an escape hatch for missing inputs.

## Immutable evaluation and canonical identity

One clean HEAD commit/tree is resolved before evaluation. Regular file contents and
executable modes are read from that immutable tree, not from mutable working files.
Cleanliness and the selected HEAD are checked again before returning computation.
The compiled evaluator and receipt source must match the selected committed blobs.
Inherited Git-local repository selectors are removed through the existing
`xtask::git_environment::isolate` helper. System/global config, fsmonitor, replacement
objects and lazy fetching are disabled for these local queries.

Commit/tree record provenance. They are intentionally excluded from the semantic
digest: an unrelated documentation commit may preserve the result. A source-byte,
file-mode, path, role, tool identity, assumption, bound, command argument, target,
feature/cfg or dependency-model change does not preserve it.

The closure map contains:

- `source/<path>` entries: committed mode, canonical role set and raw content;
- `engine/<path>` entries: evaluator/CLI/receipt/Git-isolation source and Cargo
  manifests/lockfile, automatically included rather than caller-optional;
- `profile/semantic-inputs`: the normalized specification, excluding only rollout.

Map keys use canonical sorted JSON. Declared member order and order-insensitive
feature, cfg, assumption, fairness and unresolved-input sets are normalized.
Duplicate entries in those vector sets are rejected. Command argument order remains
meaningful. Timestamps and host absolute paths are not semantic fields.

Hashes are SHA-256 with distinct versioned domains. Each payload component is
length-prefixed using unsigned 64-bit big-endian length before hashing. The closure
algorithm is exactly `formal_declared_closure.v1`; a different algorithm is stale
without an explicitly reviewed compatible migration. This does not claim
cryptographic collision impossibility.

Rollout moves such as advisory to required are deployment policy, not changed
semantics or new proof authority. Retirement is different: active computation
refuses a retired selection. Retired/superseded history is evaluated through the
receipt's explicit disposition rather than inferred from disappearing source.

## Receipt comparison and correspondence

The comparison consumes the existing `FormalProofReceipt` type and semantic
validator, not a parallel receipt schema. It rederives the recorded closure digest
from its member map before comparison. It also compares claim ID, generation, owner,
profile, method, binding, toolchain and bounds independently, so a renamed claim or
relabelled bound cannot simply wrap the old digest.

| Condition | Result |
| --- | --- |
| Valid explicit retired/superseded history | `retired` / `superseded`, without requiring deleted current source |
| Malformed or internally contradictory receipt | Refusal / `unknown` |
| Recorded digest disagrees with its member map | `unknown` |
| Unknown algorithm version | `stale` |
| Missing current computation, dirty source or unresolved declared inputs | `unknown` |
| Changed subject, binding, configuration or semantic member | `stale`, with a reason and first changed member when applicable |
| Equal declared inputs | `current`, scoped to declared-input equality only |

Each comparison also binds the exact decoded receipt using canonical JSON and the
`perl-lsp/formal-receipt-input/v1` hash domain. This is not the raw-file hash or
producer authentication. Keep the receipt and its raw bytes with the comparison;
do not pair an equal-input observation with a different native artifact record.

The historical receipt is never rewritten, reissued or upgraded. A prior native
`not_proven` result remains `not_proven` even when its inputs compare current.
Source state is independent: a dirty or unknown historical subject cannot inherit
a clean HEAD identity through matching metadata. The comparison records the current
candidate commit separately from the historical run's source commit. Do not rewrite
the receipt's original `source.commit` to reuse it across an unrelated commit: that
would falsely attribute its native execution. The admission owner must consume the
original record plus the bound comparison, preserving both provenance identities.

`correspondence_established` is always false. On an equal non-model boundary,
`correspondence_inputs_unchanged` can be true only when an explicit correspondence
member was included. That records byte/role/configuration equality, not successful
wrong-production controls, caller reachability or live LSP behavior. A model-only
receipt cannot be promoted by changing its binding label around an old closure.

The initial #19313 parent has independently owned validation repairs. This slice
consumes that owner and must not integrate before the parent is qualified. Equality
checks are not permission to ignore unresolved parent findings. A native adapter
must still authenticate production/control evidence; a caller-provided valid
receipt is never proof that a tool ran.

## Resource and platform boundaries

Input documents are limited to 2 MiB. The specification permits at most 1,024
members, with an 8 MiB per-blob and 64 MiB evaluated-source ceiling. JSON's normal
recursion limit remains enabled. Local Git queries have a 15-second deadline and
16 MiB captured-output ceiling; output goes to temporary files rather than an
unbounded pipe accumulator. A refused query is not a smaller successful closure.

The query guard waits for its direct Git child on completion/error. It is not the
native verifier process-tree supervisor and does not establish descendant custody.
The host's Git executable and local repository are trusted execution inputs; this
component does not sandbox hostile Git binaries or authenticate the host. Native
compiler/solver lifecycle and resource admission remain #19315 responsibilities.

This schema currently accepts the receipt owner's 40-hex Git identities. SHA-256
Git repositories, arbitrary dependency discovery, dirty-tree hashing and runtime
artifact authentication are unsupported. They are not approximated as clean/current.
The native platform control results remain separate from source portability claims.

## Maintenance and qualification

A substantive profile change updates its sources, roles/exclusions, bounds,
assumptions, tool/dependency identities and counterexample controls together.
Reducing a bound or excluding a previously included dependency narrows the claim;
review it as such. Preserve the prior record and compare against the newly reviewed
selection. Do not mutate an old record to pretend it was produced with the new scope.

Evaluator source or manifest/lock changes require rebuilding the binary against the
selected tree. The implicit engine entries make those changes visible. Model-only,
substrate and live-consumer bindings remain separate; adding a call-path label does
not establish a production relation.

The Rust tests exercise real temporary Git repositories, not mocked Git output:
unrelated commits, every semantic member role, modes, dirty/untracked paths,
symlinks, immutable reads and moving HEAD. Other controls cover all configuration
dimensions, explicit exclusions, canonical order, digest forgery, binding changes,
retired history, native-nonpass preservation and duplicate/unknown JSON. A literal
SHA-256 vector is independently derived and pinned in the suite.

Source review, a delimiter audit or a Python reference test is not Rust compilation
or test execution. Qualification must run the focused target above, formatting,
affected Clippy and receipt integration on the exact candidate. This slice adds no
CI workflow or gate-policy entry; the formal integration owner must route the
qualified target once the parent stack is accepted. Do not treat unexecuted tests
as maintained required evidence merely because they are present in the tree.

To complete #19314 beyond this declared-boundary slice, qualify an independently
derived complete input boundary for the real position pilot, join verified tool
and native-control artifacts, and exercise the actual candidate/result admission
consumer. No caller-controlled completeness switch may replace those checks.
