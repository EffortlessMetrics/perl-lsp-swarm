# #10138 — loaded-module reload wire family (R01B)

Canonical JSON vectors binding the registered `perl-lsp/loadedModuleReload`
version 1 custom DAP family to one executable expectation corpus consumed by
both sides of the wire:

- Rust (adapter side): `crates/perl-dap/src/reload_family.rs` runs every
  vector through the negotiation/evaluation/projection model.
- TypeScript (client side): `vscode-extension/src/test/loadedModuleReloadFamily.test.ts`
  runs every vector through the generated projection in
  `vscode-extension/src/loadedModuleReloadFamily.generated.ts`.

## Scope

Registration and transport compatibility only (#10138, under the frozen
semantic contract of #10097 / ADR-0046 and the classification authority of
#6737 / #4838):

- the family is registered in `.ci/dap/protocol-authority.json`
  (`project_families`), namespaced and versioned, never colliding with a
  standard DAP request name the adapter dispatches;
- it is **not dispatched** (`dispatch.rs` `SUPPORTED_COMMANDS` pins this) and
  **not advertised** in any capability until the R04 exact-proof leaf;
- the wire terminal vocabulary is the frozen #10097 vocabulary projected
  verbatim (`reloaded`, `refused`, `failed_before_mutation`,
  `indeterminate_possibly_applied` plus the frozen disposition/phase/cause
  codes); transport never redefines whether runtime state changed, and
  `indeterminate_possibly_applied` is never flattened to a clean failure;
- the request payload is the typed, adapter-issued opaque subject only; raw
  paths, debugger commands, and Perl expressions are refused;
- unknown fields, unknown enum variants, and unknown versions fail closed
  under the registry-recorded v1 policy;
- bounds (bytes, identities, reasons, details) are enforced before
  publication, and responses carry stable codes plus content-addressed or
  opaque identities only.

## Vector shape

Every fixture is one JSON document:

```text
schema                 — perl_dap.loaded_module_reload_family.vector.v1
name                   — stable vector name
negotiation?           — client declaration + adapter session state used to
                         negotiate before evaluation (absent client object =
                         a client with no family support)
request?               — wire request document (projection-only vectors omit)
outcome?               — frozen #10097 outcome document (kind, and where the
                         kind carries them phase/disposition/cause)
generation_before?     — runtime-module generation clock seed
expect                 — exact expected projection: evaluation admitted or
                         rejected with a typed code, response kind, DAP
                         success flag, possibly_applied, generation witness,
                         reconciliation dispositions, or the client-side
                         fail-closed classification for unknown variants
```

## Drift policy

A hand-written TypeScript interface or Rust wire type that diverges from
this corpus, the wire schema
(`schemas/loaded_module_reload_family.v1.schema.json`), or the registry entry
fails the consuming test on either side. The corpus is descriptive of the
frozen #10097 vocabulary only; it adds no semantics.
