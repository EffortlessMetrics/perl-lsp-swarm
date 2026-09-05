# Zed official-registry `perl-dap` journey receipts

> **State:** validator authority landed; the public journey itself is
> `blocked_external`. No released official `zed-industries/extensions` entry
> serves the DAP-capable existing `perl` extension yet, #9486 has no current
> exact-source real Zed DAP receipt, and #9483's routing/final-check authority
> is not current. The committed receipt records exactly those absent gates.
>
> **Owner:** [#9487](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9487)
>
> **Asset authority:** [#9516](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9516)
> (landed via #12333; checked contract plus committed aggregate receipt)

This stage proves the official-registry public DAP journey — the ordinary
`perl` extension installed from the official Zed registry driving the managed
public `perl-dap` artifact through a real Zed debug session. It consumes the
#9516 projection surface and never re-derives it:

```text
official registry install (clean profile, no dev/fork/override/PATH/prior cache)
  < managed resolution selects the exact #9516 release/target/asset/member/binary
  < real Zed session: breakpoint verified, stopped at the intended source,
    bounded frame/scopes/continue cells, terminate/disconnect, no orphans
  < restart reuses the exact verified known-good managed subject
```

Separation is absolute, in both directions:

- the #9486 exact-source dev-extension stage cannot satisfy this validator
  (`stage`/`install_route` are bound to `public_registry_install` /
  `official_registry`);
- the #9516 public asset receipt cannot satisfy this validator (bytes and
  process smoke are not real Zed debugger behavior; the receipt schema, Zed
  host identity, and journey cells are required);
- this stage cannot satisfy #9486 or #9516, and it changes no LSP support row.

## Consumed authorities

| Subject | Authority |
| --- | --- |
| managed asset selection, digests, cache boundary | checked contract `.ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json` |
| current public asset evidence | committed aggregate receipt `.ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json` (validated by the #9516 validator itself) |
| merged-and-released official registry subject | DU01 acceptance manifest `.ci/fixtures/zed-perl-upstream/registry/manifest.toml` |
| exact-source gate accounting | committed `exact-source*.json` receipts |

The receipt binds the contract and aggregate receipt by recomputed sha256, so
any drift on either surface invalidates the journey receipt offline. The
registry gate is evaluated with the DU01 predicate from the implementation
train (changed branch-reachable commit/version, manifest-version equality,
released-build containment) — submission or merge metadata alone never
satisfies it.

**Boundary on the #9516 residual:** the owed linux/macOS matching-host
executions are owned by the #9516 closeout residual (run `execute-dap` on
those verifiers and commit `dap-asset-<host>.v1.json` rows). This stage is a
different consumer: it accepts `managed_extracted_not_executed` rows as valid
binding evidence for a non-Windows journey host while requiring the aggregate
receipt to be a current pass. It never re-runs or re-derives the asset matrix,
and it does not own the recorded Windows member divergence (#9485 and #7980
own that).

## Static checks

Offline, suitable for pull requests (on Windows use `python`):

```bash
python3 scripts/zed_dap_asset_receipts.py validate-dap-public-receipt \
  --receipt .ci/fixtures/zed-perl-upstream/receipts/dap-public-registry.v1.json \
  --contract .ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json \
  --asset-receipt .ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json \
  --registry-manifest .ci/fixtures/zed-perl-upstream/registry/manifest.toml
```

The xtask falsifier surface is
`cargo test -p xtask --test zed_dap_public_registry_receipt --locked`.

## Validator rejections

A passing receipt is rejected when any of these hold:

- an exact-source or asset-receipt stage/shape is relabeled public;
- the registry subject disagrees with the accepted DU01 manifest (dev fork,
  copied package, wrong registry, wrong commit/version/build);
- any clean-profile precondition is unobserved (dev extension, prior public
  extension state, prior managed `perl-dap` cache, explicit debugger binary
  override, PATH candidate, another Perl debugger, relabeled receipt);
- the adapter route is not `managed_public_artifact`, the process path is not
  the managed installed path, the argv names the `perllsp` product, or the
  version line is not the exact canonical `perl-dap <version>` line;
- the wrong-root same-basename discriminator is not observed rejected;
- any journey cell (host identity through restart reuse) is not proven with
  evidence;
- the managed cache before-inventory is non-empty, the after-inventory is not
  exactly the selected subject, or the restart does not reuse it without a
  second provider;
- the adapter exit leaves an adapter or debuggee orphan;
- the journey platform has no managed contract row (unsupported platforms
  cannot be promoted by inference), or the selected target is not the row
  matching the platform;
- the #9516 contract or aggregate receipt digests drifted.

A `blocked_external` receipt is rejected when any gate it names is actually
current: a published DU01 subject, a current #9516 pass it denies, or a
committed exact-source pass it denies. When the registry subject lands, the
committed blocked receipt fails validation until it is regenerated — the
honest wake event for this stage.
