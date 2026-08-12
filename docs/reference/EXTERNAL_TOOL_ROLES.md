# External Tool Roles and Native Replacements

The machine-readable authority is
`perl_lsp_rs_core::external_tools::EXTERNAL_TOOL_REGISTRY`.

This page explains the policy projection. Domain-specific compatibility
registries remain authoritative for formatter options, critic policies, and
debugger-peer capabilities.

## Product rule

`perl-lsp` ships its native stack. External Perl implementations are not bundled
and are not required for normal operation.

```text
native product                  external role
──────────────────────────────  ─────────────────────────────────────
perllsp + perl-dap              PLS conformance comparison only
native formatter                .perltidyrc compatibility, explicit adapter,
                                repository conformance
native critic                   .perlcriticrc compatibility, explicit adapter,
                                repository conformance
perl-dap                        explicit optional ptkdb peer
```

PATH or module presence may identify an advisory candidate where authorized. It
never selects an engine or executes a tool on workspace open.

## Reviewed entries

### Perl::LanguageServer

```text
native replacement: perllsp + perl-dap
role: repository-only conformance oracle
bundled: false
required for native: false
auto detect: false
auto select: false
runtime adapter: false
install help: developer/conformance setup only
owner: #6956 / #7210
```

PLS is not a supported language-server fallback or DAP backend. Any retained
comparison runner lives outside published product source and binds exact tool,
Perl, fixture, configuration, and native candidate identity.

### Perl::Tidy / perltidy

```text
native replacement: native formatter
roles: configuration compatibility, explicit external adapter, conformance
config file: .perltidyrc
bundled: false
required for native: false
auto select: false
external execution: explicit user selection only
owner: #7056 / #7134 / #7135
```

Reading a profile does not switch engines. Unsupported or external-only options
remain visible rather than silently changing the native claim.

### Perl::Critic / perlcritic

```text
native replacement: native critic
roles: configuration compatibility, explicit external adapter, conformance
config file: .perlcriticrc
bundled: false
required for native: false
auto select: false
external execution: explicit user selection only
owner: #6997 / #6987 / #7211
```

The native critic remains selected when a profile or executable is discovered.
The compatibility reader maps only behavior-backed policy identities.

### Devel::ptkdb

```text
native host: perl-dap
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
- automatic engine selection;
- execution on workspace open;
- an explicit external adapter without the process/trust owner;
- PLS exposed as a product execution adapter;
- duplicate canonical identities or aliases.

The registry serializes deterministically for doctor, docs, settings, readiness,
and package-check consumers.

## Consumer boundary

Consumers should use the common policy for:

- native replacement and role labels;
- whether absence is a health failure;
- whether auto-detection or selection is permitted;
- install-help scope;
- common package-exclusion identities;
- claim ownership.

They must not move domain details into the common registry. For example:

- `.perltidyrc` option dispositions stay with the formatter compatibility owner;
- Perl::Critic policy aliases and parameters stay with the critic registry;
- ptkdb request/event capabilities come from the authenticated session;
- process identity, trust, environment, and execution stay with the environment
  and process controllers.
