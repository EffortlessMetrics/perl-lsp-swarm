# Public Zed registry receipt

> **State:** not run; blocked pending upstream merge, registry publication, and released defaults.
>
> **Owner:** [#7912](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7912)

This is the first lane allowed to promote a public Zed support cell. It begins
only after the existing `perl` extension is published through Zed's official
registry and the required default server ordering is available in a released
Zed build.

## Clean-install journey

1. Start with a fresh Zed profile with no development extension, prior Perl
   extension, managed cache, or `perllsp` binary override.
2. Install the official `perl` extension from the registry.
3. Open Perl under released defaults and prove only the intended default
   provider starts; `perl-lsp` and `perllsp` remain quiet.
4. Explicitly select `perllsp` and prove the extension downloads the exact
   public target asset.
5. Bind the asset digest, process path, version, and exact `perllsp --stdio`
   arguments.
6. Repeat the bounded semantic, activation, configuration, Unicode, freshness,
   restart, and shutdown journey from the exact-source receipt.
7. Restart and prove the known-good managed cache is reused.
8. Disable `perllsp` and prove the process exits cleanly.

## Exact public subject

The receipt keeps these identities separate:

```text
released Zed version/build and source defaults
zed-industries/extensions commit and [perl] version
merged tree-sitter-perl/zed-perl commit and package identity
public perllsp release, target, asset URL, and SHA-256
clean profile and prior-cache state
fixture and configuration digests
```

The templates remain under
`.ci/fixtures/zed-perl-upstream/receipts/`. They intentionally contain no future
publication identities and cannot be promoted in place without actual evidence.

## Route boundary

Managed download and a user-installed PATH binary are independent rows. A PATH
receipt cannot prove the managed asset, and the managed route cannot prove an
arbitrary package-manager or user installation.

## Promotion boundary

After a passing run, #7912 may promote only the exact Zed version, platform,
extension version, `perllsp` release/target, install route, activation families,
configuration behavior, and journey cells observed. Other platforms, versions,
methods, routes, and DAP remain unproven or unsupported.

A public `result: "pass"` receipt must bind
`public_subject.relative_path` plus the content SHA-256 of the published subject
document and must use `resolution_route=managed_download` with
`prior_extension_absent` and `prior_managed_cache_absent` both true. Development
binary overrides, worktree PATH installs, and warm managed caches cannot validate
as official-registry evidence.
