# perl-subprocess-runtime

Shared subprocess execution primitives for Perl LSP ecosystem crates.

The crate takes no runtime dependencies, which is deliberate: a process
substrate that cannot acquire an LSP, DAP, formatter, or test-framework type
also cannot be pulled into a dependency cycle by one of its consumers.

## Supervised process domain (`process`)

The current contract. A declarative, versioned plan is validated once, and the
validated form is the only thing a supervisor will start:

```text
domain operation + exact identities + authorization evidence
  -> ProcessPlan
  -> validate()            (pure; the only route to a startable plan)
  -> ValidatedProcessPlan
  -> ProcessSupervisor     (port)
  -> ordered ProcessEvent stream
  -> terminal ProcessResult
```

It carries what a caller actually needs to be honest about a subprocess:
exact executable and working directory, a declarative environment projection,
opaque authorization evidence, per-channel capture budgets that keep observed
and retained bytes distinct, a deadline, a cancellation policy, a termination
policy that distinguishes signalling the immediate child from reaping a process
group, and a closed set of terminal states in which a timeout can never read as
an ordinary success.

The domain performs **no** operating-system spawn. The only supervisor it ships
is `process::FakeSupervisor`, a deterministic in-memory fake for consumer
tests; every result it produces is marked `EvidenceClass::Fake` and carries
`Limitation::FakeEvidenceOnly`. Real execution is the next node of the shared
process train.

Nothing in the domain claims sandboxing, isolation, or hermeticity.

## Legacy seam

- `SubprocessRuntime` trait
- `OsSubprocessRuntime` implementation (non-WASM)
- `mock` module for deterministic tests

These predate the domain and remain because live consumers compile against
them. They are closed to new consumers: see `process::legacy` for what they
cannot express and who owns their removal.
