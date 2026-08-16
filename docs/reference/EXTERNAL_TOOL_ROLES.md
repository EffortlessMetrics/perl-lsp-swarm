# External Tool Roles and Native Replacements

The machine-readable authority is
`perl_lsp_rs_core::external_tools::EXTERNAL_TOOL_REGISTRY`.

This page explains the policy projection. Domain-specific compatibility
registries remain authoritative for formatter options, critic policies,
import-cleanup semantics, and debugger-peer capabilities.

## Product rule

`perl-lsp` ships its native stack. External Perl implementations are not bundled
and are not required for normal operation.

```text
native host                     external role
──────────────────────────────  ─────────────────────────────────────
perllsp + perl-dap              PLS repository conformance only
perllsp                         .perltidyrc compatibility, explicit adapter,
                                conformance
perllsp                         .perlcriticrc compatibility and repository
                                conformance only — no product adapter
perllsp                         explicit perlimports adapter, conformance
perl-dap                        explicit optional ptkdb peer
```

PATH or module presence may identify an advisory candidate where authorized. It
never selects an engine or executes a tool on workspace open.

The registry deliberately expresses **different policies per domain**. Perl::Tidy
and App::perlimports own explicitly selected product adapters. Perl::Critic and
Perl::LanguageServer do not, and validation prevents them from gaining one.

## Native replacement identity

Each entry names its native replacement by exact identity rather than prose, so
documentation, doctor, packaging, and settings cannot drift into separate
vocabularies:

```text
products / package / library / executable / lsp_consumer
delivery: shipped | planned | not_applicable
current_implementation (when the ruled identity has not shipped)
owner
```

`delivery: planned` is load-bearing honesty. It records that a maintainer ruling
exists without implying the identity already ships.

## Reviewed entries

### Perl::LanguageServer

```text
native replacement: perllsp + perl-dap (shipped)
role: conformance oracle
external execution: repository conformance only
bundled: false
required for native: false
auto detect: false
auto select: false
runtime enablement: forbidden
install help: developer/conformance setup only
owner: #6956 / #7210
```

PLS is not a supported language-server fallback or DAP backend. Any retained
comparison runner lives outside published product source and binds exact tool,
Perl, fixture, configuration, and native candidate identity.

### Perl::Tidy / perltidy

```text
native replacement:
  products:   perl-tidy
  package:    perl-tidy
  library:    perl_tidy
  executable: perl-tidy
  consumer:   perllsp
  delivery:   planned (#7411 / #8653 / #7143)
  current:    perl-lsp-perltidy
roles: configuration compatibility, explicit external adapter, conformance
config file: .perltidyrc (reader owner #8509)
external execution: explicit product adapter (owner #7134)
candidate output validation: #7056
runtime enablement: explicit user action
owner: #7056 / #7134 / #7135
```

`perl-tidy` / `perl_tidy` is the ruled canonical native identity per #7411.
`perl-lsp-perltidy` is the historical identity currently implementing it.

Reading a profile does not switch engines. Unsupported or external-only options
remain visible rather than silently changing the native claim. The registry
mechanically rejects any attempt to name Perl::Tidy itself as the canonical
native formatter.

### Perl::Critic / perlcritic

```text
native replacement: native critic, delivery planned (#8253 / #9062 / #9068)
roles: configuration compatibility, conformance oracle
config file: .perlcriticrc (reader owner #7211)
external execution: repository conformance only (owner #6987 / #7210)
runtime enablement: forbidden
auto detect: false
install help: developer/conformance setup only
trust class: repository conformance
owner: #6997 / #7211 / #8253
```

Perl::Critic has **no first-party external runtime, editor, or CLI mode**.
`.perlcriticrc` is parsed process-free into an explained native plan. A pinned
real `perlcritic` may execute only from repository/developer conformance
entrypoints. PATH or config-file presence never changes native runtime
behavior, and oracle success or failure never changes product readiness or
diagnostics.

This is the specific regression #7209 exists to prevent, so it is enforced by
validation rather than convention.

### App::perlimports / perlimports

```text
native replacement: import cleanup planner, delivery planned (#8277)
roles: explicit external adapter, conformance oracle
external execution: explicit product adapter (owner #8277)
source requirement: saved file
candidate output validation: #8277
auto detect: advisory only
auto select: false
runtime enablement: explicit user action
owner: #8277
```

External output is candidate evidence only. It never bypasses the #8277
source and edit-safety authority, and the registry rejects an adapter whose
output would become edit authority without a named validation owner.

### Devel::ptkdb

```text
native host: perl-dap
native replacement: not applicable
role: explicit optional debugger peer
bundled: false
required for native: false
auto detect/select: false
perl-dap remains DAP server: true
owner: #4786 / #7276
```

ptkdb is not a fallback DAP server. Bootstrap and experimental live-peer claims
remain separately bounded by their evidence.

## Registry invariants

Validation rejects:

- any bundled external implementation;
- any external tool required for native behavior;
- a native package that depends on an external implementation;
- automatic engine selection;
- execution on workspace open;
- enablement merely by discovery;
- disagreement between a role and its support class;
- an external adapter without the process/trust owner;
- an authorized execution class without an environment/process owner;
- an adapter whose output could become an edit without a validation owner;
- a conformance oracle without a pinned version and bounded receipts;
- external evidence promoting native readiness;
- configuration-file presence authorizing execution;
- a configuration reader without a domain owner;
- a debugger peer without the peer trust owner;
- an execution or peer role without explicit user enablement;
- PLS exposed as product runtime;
- Perl::Critic gaining a product runtime, editor, or CLI adapter, user-facing
  install help, or ordinary-startup detection;
- a native replacement that names the external tool itself;
- a shipped native replacement with no exact identity;
- a native replacement without a delivery owner;
- duplicate or empty canonical identities and aliases.

The registry serializes deterministically for doctor, docs, settings, and
readiness consumers.

## Identity aliases are not package patterns

Aliases such as `pls`, `perltidy`, `perlimports`, and `ptkdb` exist for exact,
case-insensitive identity resolution. They are deliberately not exported as
package deny-list substrings. A raw archive scan for `pls`, for example, would
also match unrelated filenames and documentation.

Package and release checks must own artifact-specific rules instead:

```text
exact path or basename
expected file type
allowed repository-only location
published-package inclusion boundary
```

That keeps detection vocabulary separate from proof that an external executable,
module, or bundled runtime entered a release artifact.

## Consumer boundary

Consumers should use the common policy for:

- exact native replacement identity and role labels;
- whether absence is a health failure;
- whether auto-detection or selection is permitted;
- external execution class and its owner;
- install-help scope;
- exact identity resolution;
- claim ownership.

They must not move domain details into the common registry. For example:

- `.perltidyrc` option dispositions stay with the formatter compatibility owner;
- Perl::Critic policy aliases and parameters stay with the critic registry;
- import-cleanup plan semantics and edit safety stay with #8277;
- ptkdb request/event capabilities come from the authenticated session;
- package payload rules stay with the package/release controller;
- process identity, trust, environment, and execution stay with the environment
  and process controllers.
