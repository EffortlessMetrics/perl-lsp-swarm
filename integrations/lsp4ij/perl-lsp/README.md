# Perl LSP4IJ template candidate

This directory is the repository-owned **desired LSP4IJ Perl language-server template** used for local interoperability proof and upstream-delta preparation.

It is not evidence that a released LSP4IJ build already contains these files, and repository automation must not submit them upstream automatically.

## Configuration authority

The server-native configuration namespace is `perl.*`. The checked projection in `settings.schema.json` is a deliberately small LSP4IJ-facing subset of `schemas/perllsp-settings.schema.json`.

`settings.json` is intentionally empty. Defaults in the schema describe server behavior; copying them into the transmitted settings object would turn implicit defaults into explicit high-precedence editor overrides and could mask `.perl-lsp.toml` project configuration.

Keep these responsibilities separate:

- `.perl-lsp.toml` — portable project/repository configuration;
- LSP4IJ **Configuration** tab — sparse user/editor overrides expressed as `perl.*` keys;
- `initializationOptions.json` — values genuinely required during initialize, not ordinary live settings;
- `installer.json` — binary acquisition and platform selection, owned by #7876;
- LSP4IJ client/trace settings — client behavior rather than server configuration;
- Perl DAP material — separate debugger integration and evidence.

## Current bounded surface

The template activates only directly governed ordinary Perl families:

- `*.pl`
- `*.pm`
- `*.t`

Other Perl-adjacent and mixed-language families must earn independent activation and semantic evidence before they are added.

Automatic format-on-save is intentionally absent from the LSP4IJ settings projection. Ordinary document/range formatting remains available through standard LSP methods; format-on-save requires a real LSP4IJ/IntelliJ client mechanism to be proven separately.

## Local proof

A supported released LSP4IJ build can use this directory through **Import from custom template...** once the installer material from #7876 is composed with it. That local-import subject is distinct from both the currently released built-in Perl template and any future upstream-released corrected template.

## Maintainer workflow

The desired integration is downstream product state, not an independent source of truth. Changes to configuration, CLI identity, release topology, file-family evidence, or LSP4IJ upstream state must be reconciled through their canonical authorities before this directory is refreshed.

See [`docs/development/LSP4IJ_MAINTENANCE.md`](../../../docs/development/LSP4IJ_MAINTENANCE.md) for the status/check/refresh/prepare-delta contract, evidence invalidation rules, manual upstream handoff, and the distinction between local-ready, submitted, merged, released, and released-and-receipted states.

External submission remains a deliberate maintainer action. Repository automation may prepare a bounded local delta; it must not create or push an upstream branch, open/comment on an upstream issue or PR, or merge anything upstream.
