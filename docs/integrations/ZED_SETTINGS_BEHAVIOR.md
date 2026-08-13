# Zed settings behavior receipt

> **State:** contract and validator implemented; no settings behavior is proven until the exact host experiments pass.
>
> **Owner:** #7990. Parent: #7902. Host driver: #7984.

The static Zed integration already preserves the intended shape:

```text
lsp.perllsp.binary
  Zed process selection

lsp.perllsp.settings.perl
  canonical server-native perl.* settings
```

This lane proves whether real Zed transports, applies, supersedes, removes, and refreshes those settings. Serialization alone is not evidence of consumption.

## Canonical probes

The checked experiment contract is:

```text
.ci/fixtures/zed-perl-upstream/settings-behavior.v1.json
```

It consumes `schemas/perllsp-settings.schema.json` and covers:

| Type | Canonical key | Observable discriminator |
|---|---|---|
| Boolean | `perl.inlayHints.enabled` | hints visible, then absent |
| Enum | `perl.critic.profile` | strict-only diagnostic |
| Integer | `perl.formatting.maximumLineLength` | formatting output at widths 100 versus 40 |
| Path/list | `perl.workspace.includePaths` | mutually exclusive module resolution roots |
| Nested | `perl.aiCompletion.streaming.enabled` | exact winning value in the server configuration trace |

The nested probe proves configuration ownership only. It does not promote editor-visible AI completion.

## Required host roles

Use the exact-source driver from #7984 to produce four independently validated `zed_host_compat.v1` receipts against one host, extension, binary, platform, and fixture subject:

```text
project_only
  .perl-lsp.toml supplies the project value; no conflicting Zed override

zed_override
  Zed settings supply the conflicting value

zed_override_removed
  the Zed override is removed and project behavior returns

live_edit
  one Zed setting changes while the session is running
```

Each role records its own settings digest and receipt digest. All roles share one `host_identity_sha256`; a receipt from another Zed build, extension tree, binary, platform, or fixture cannot satisfy the experiment.

## Precedence proof

For every probe, retain:

```text
project_observed == project_value
zed_override_observed == zed_value
restored_observed == project_value

effect_before != effect_override
effect_before == effect_restored
```

The include-path fixture should contain one module reachable only from `project-lib` and another reachable only from `zed-lib`. That prevents a broad or accidental search path from satisfying both phases.

A JSON object appearing in a log is insufficient. Each probe needs a behavior delta or, for the nested trace-only probe, an exact configuration-owner/value observation.

## Live or restart behavior

Record exactly one passing disposition:

```text
live_configuration
  Zed sends a configuration notification, the exact server PID remains stable,
  and behavior changes.

zed_managed_restart
  Zed replaces the exact server process and behavior changes.

manual_restart
  behavior changes only after the documented manual restart and the exact server
  PID changes.
```

`no_effect` and `instrument_failed` remain valid observations but cannot produce a passing settings receipt.

## Receipt

Start from:

```text
.ci/fixtures/zed-perl-upstream/receipts/settings-behavior-template.json
```

Bind the experiment contract digest, the four exact host receipt identities, direct evidence for every probe, the reversible precedence sequence, and the restart disposition. Then validate:

```bash
cargo run -p xtask --bin validate-zed-settings-behavior -- \
  .ci/fixtures/zed-perl-upstream/settings-behavior.v1.json \
  schemas/perllsp-settings.schema.json \
  /path/to/settings-behavior-receipt.json
```

A passing receipt requires:

- exact canonical-schema agreement;
- no `binary.path`, arguments, or environment leakage into server settings;
- all required host roles and content-addressed identities;
- reversible before/override/restored effects for every probe;
- project → Zed override → project restoration precedence;
- a directly observed live-or-restart disposition;
- `full_zed_support = not_proven` and `public_registry = not_proven`.

## Limits

This receipt proves configuration behavior only for its exact host subject. It does not prove managed download, dormant-default ordering, the complete semantic journey, official-registry installation, or public Zed support.
