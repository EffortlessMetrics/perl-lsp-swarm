# `perllsp` Mason registry submission packet

External target: `mason-org/mason-registry`  
Upstream path: `packages/perllsp/package.yaml`  
Neovim config identity: `perllsp` from #7722

The staged package in this directory is preparation evidence only. Do **not** represent it as public Mason availability until the external package is accepted and observable through the public registry.

## Current staged subject

The candidate is pinned to the currently public `EffortlessMetrics/perl-lsp` v0.17.0 release so its package shape can be reviewed and tested against real release assets now. Before external submission, update the `source.id` to the newest public release whose exact artifacts are accepted by the release/public-artifact evidence and rerun the complete local Mason test.

The current public archive topology supplies:

```text
darwin_arm64
  perllsp-<version>-aarch64-apple-darwin.tar.gz

darwin_x64
  perllsp-<version>-x86_64-apple-darwin.tar.gz

linux_arm64_gnu
  perllsp-<version>-aarch64-unknown-linux-gnu.tar.gz

linux_x64_gnu
  perllsp-<version>-x86_64-unknown-linux-gnu.tar.gz

linux_arm64_musl
  perllsp-<version>-aarch64-unknown-linux-musl.tar.gz

linux_x64_musl
  perllsp-<version>-x86_64-unknown-linux-musl.tar.gz

win_x64
  perllsp-<version>-x86_64-pc-windows-msvc.zip
```

The archives contain a top-level `perllsp-<version>-<target>/` directory and the canonical `perllsp` executable beneath it. Do not add an unsupported target merely because Mason has a target identifier for it.

## Admission gates

Before opening the external Mason PR, record at least one current Mason admission route:

```text
>= 100 GitHub stars
OR >= 5000 VS Code Marketplace downloads
OR approved in nvim-lspconfig
OR an accepted official recommendation route
```

The intended route for this package is #7722 upstream `nvim-lspconfig` approval unless another criterion is independently satisfied first.

Also require:

- #7722 has established the exact upstream `perllsp` lspconfig identity before retaining `neovim.lspconfig: perllsp` in the submitted package;
- #7770 has a public-Mason installed-binary/actual-Neovim receipt for the exact package subject or records that row as not yet proven;
- the selected public release exists and every declared target asset is observable;
- `perllsp --version` from a locally installed Mason package agrees with the package source version;
- the staged package passes `check_mason_perllsp_candidate.py`;
- Mason's own current schema/local-registry test passes for every claimed target.

## Local Mason procedure

Follow Mason's current `CONTRIBUTING.md` local-registry route rather than inventing a repository-specific installer:

1. clone `mason-org/mason-registry` at a reviewed ref;
2. copy `packages/perllsp/package.yaml` from this packet into that checkout;
3. configure a clean `mason.nvim` profile to use the checkout via a `file:` registry;
4. validate the package with Mason's current package/schema tooling;
5. exercise supported targets with `:MasonInstall --target=<target> perllsp` where the host/tooling can emulate them;
6. for the native host target, run the installed `perllsp --version` and `perllsp --health`;
7. run #7770's installed-binary Neovim journey using the Mason prefix, with ambient/workspace `perllsp` excluded from the subject;
8. uninstall/reinstall once to ensure registry state and executable resolution remain deterministic.

## External submission procedure

1. Update the staged source to the latest accepted public release if v0.17.0 is no longer the intended public subject.
2. Re-run local schema/install/actual-host proof.
3. Confirm one current admission criterion is satisfied.
4. Open the Mason PR with the single package plus only upstream-required generated files.
5. After merge, record the upstream merge commit and first public-registry state in #7730/#7122.
6. Only then allow #7736 to advertise `:MasonInstall perllsp` as a public user path.

A local registry, fork, candidate file, or successful release-archive smoke is not public Mason distribution evidence.
