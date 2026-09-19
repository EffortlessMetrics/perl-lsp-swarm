# Context: #11521 — standalone PATH persistence and fresh-process resolution

## Problem

PATH carries three distinct propositions that the standalone install surfaces
have historically collapsed into one:

```text
selected candidate has an exact executable path
≠ profile or registry state was changed correctly
≠ a genuinely new ordinary process resolves that exact candidate
```

Nothing in the tree kept them apart. `standalone_install_transition.v1` carries a
`path_persistence` outcome dimension (`persisted` / `unchanged` / `failed` /
`not_applicable`) that #11493's diagnostics registry consumes, but that is a
single reported word with no contract behind it: no document says what a
persistence mutation is allowed to touch, who owns the entry, which scopes are
distinct, or what evidence would make a discoverability claim honest. A
current-process PATH edit, an absolute-path launch, or a successful
installer-child execution could each be reported as `persisted` without
contradicting anything.

That gap is what lets a green install receipt coexist with a user whose next
shell cannot find `perllsp`.

## Status: contract packet and checked validator this lane; no platform mutation

This lane defines the versioned semantics, the plan/result documents, the
validator, and the deterministic fixtures. It mutates no shell profile, no
registry, no environment, no install root, and no current process, and it
launches no hosted fresh-process proof. The POSIX profile/PATH and Windows
User-PATH adapters under #7832 implement the mechanics against these semantics;
hosted proof is #10746.

## Current-main facts the builder consumed

- `xtask/src/standalone_diagnostics.rs:105` —
  `PATH_PERSISTENCE: &[&str] = &["persisted", "unchanged", "failed", "not_applicable"]`,
  a reported outcome dimension of the install transition, not a persistence
  contract. This lane does not change it; the diagnostics registry keeps
  reporting transition outcomes, and this packet defines what may produce them.
- `xtask/examples/standalone_owned_state.rs` and
  `schemas/standalone_owned_state.v1.schema.json` (#11470) — the established
  shape for a standalone contract packet: closed enums, a plan bound to its
  subject by digest, a result bound to its plan, an example-binary validator
  with an embedded battery, committed fixtures, and a schema-only parity
  harness. #11521 follows it rather than inventing a second shape. The
  `path_marker`, `profile_marker`, and `registry_marker` roles that #11470's
  owned-state manifest already enumerates are the state this packet plans the
  creation of.
- `plans/install-route-classification/implementation-plan.md` names
  "PATH/session/execution" as one of seven install-route dimensions and defers
  the cross-cutting PATH fan-in to #11443 under #10334. It specifies no types
  and is a provisional planning document, not a contract input. Public route
  classification remains #10336's; this packet deliberately publishes no route
  word.
- No `standalone_path_*` or `standalone_fresh_process*` schema, module,
  example, fixture directory, or `.spec/` entry existed on `origin/main`.

## Why this approach

Three documents rather than one, because the propositions fail independently and
must be separately falsifiable:

- a plan can be well-formed while the write fails;
- a write can succeed while a new shell still resolves an older binary;
- a fresh process can resolve the candidate while the plan never authorized the
  entry that made it visible.

Collapsing them into one receipt would reintroduce exactly the conflation the
issue exists to remove. Keeping persistence and observation separate is also
what lets the POSIX and Windows adapters implement different mechanics without
renegotiating semantics.

The validator is an example binary rather than a library module because, like
#11470, its consumers are installer adapters and CI rather than other xtask
tasks, and because the battery is the deliverable.

## Alternatives rejected

- **One combined `standalone_path.v1` document.** Cheaper to write, but a single
  `result` word would have to mean both "the profile was edited" and "a new
  shell finds it", which is the defect.
- **Reusing `standalone_install_transition.v1`'s `path_persistence` dimension as
  the contract.** It is a reported outcome consumed by the diagnostics registry;
  giving it contract authority would put policy, mutation evidence, and
  observation evidence behind one enum with no inputs bound to it.
- **Rust types only, no published JSON Schema.** The adapters that will perform
  these mutations are POSIX shell and PowerShell. A law that only the Rust
  validator enforces is a law those adapters cannot check.
- **Free-text diagnostic fields.** Free text in a durable receipt defeats
  bounded-field redaction; a complete PATH or profile value pasted into a
  "reason" string is exactly the leak the contract forbids.

## Links

- Parent/controller: #7832 — platform adapters follow.
- Programme: #10703.
- Predecessor: confirmed standalone candidate from #8359; transaction authority
  #10243.
- Sibling contract packets: #11470 (owned state, landed), #11493 (diagnostics,
  landed).
- Hosted proof: #10746. Route evidence: #10334. Public route classification:
  #10336. Uninstall/rollback: #8372 / #8359.

## Scope boundary

No POSIX profile or Windows registry mutation. No universal shell or profile
manager. No installed/public route classification. No release or publication
action. No change to the landed `standalone_install_transition.v1` vocabulary or
to #11493's diagnostics registry.
