# Competing Perl tooling (coexistence guidance)

Users may run `perl-lsp` alongside other Perl language servers, formatters, critic
providers, or debuggers. The product explains high-confidence overlaps where it can
observe them and keeps remediation advisory. It never disables, uninstalls, rewrites, or
impersonates another tool ([#7214](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7214)).

## VS Code and VS Code–compatible editors

The `EffortlessMetrics.perl-lsp-rs` extension performs **bounded** coexistence detection on
the VS Code host:

- reviewed extension identities (exact id match only, no name scanner);
- selected `perl-lsp.*` and `[perl]` editor settings;
- declared `perl` debugger contributions;
- workspace `.perltidyrc` presence as residue evidence, not a provider.

Use **Show Coexistence Status** (`perl-lsp.showCoexistenceStatus`) for the current
finding set, evidence sources, registry reason codes shared with doctor (#7209/#7212), and
bounded remediation choices. Startup may show one advisory notification per distinct
finding set; suppressions bind to an exact conflict identity.

PATH presence of `perlcritic` or `perltidy`, and a project `.perlcriticrc` file alone, are
**not** reported as external providers. First-party `perl-lsp` has no selectable external Perl::Critic engine in the extension; stale legacy critic settings route to migration
guidance instead.

See also [VS Code Setup](VS_CODE_SETUP.md).

## Other editors and generic LSP clients

Neovim, Emacs, Helix, Vim/coc.nvim, Sublime, IntelliJ/LSP4IJ, Zed, and other non–VS Code
integrations **do not** run the VS Code coexistence detector. Without the extension host
APIs, the product cannot honestly claim universal conflict detection on those surfaces.

What you can use instead:

- **`perllsp --health`** and server doctor output for native readiness and optional-tool
  posture (#7212), using the same reviewed external-tool roles as the editor (#7209).
- **Your editor’s own configuration** to see which language server, formatter, diagnostics,
  and debugger integrations are enabled. Resolve overlaps manually; do not expect
  `perl-lsp` to mutate another product.
- **Editor-specific setup guides** below for integration boundaries and honest limits.

Do not infer competing providers from `PL*` spelling, PATH alone, or config-file residue.
When coexistence is limited on a host, document the winning integration and the specific
setting or extension to adjust; do not rank tools by quality.

| Editor family | Setup guide |
| --- | --- |
| Neovim built-in LSP | [NEOVIM_SETUP.md](NEOVIM_SETUP.md) |
| Vim / coc.nvim | [VIM_SETUP.md](VIM_SETUP.md), [COC_NEOVIM_SETUP.md](COC_NEOVIM_SETUP.md) |
| Emacs | [EMACS_SETUP.md](EMACS_SETUP.md) |
| IntelliJ / LSP4IJ | [INTELLIJ_IDEA_SETUP.md](INTELLIJ_IDEA_SETUP.md) |
| Helix | [HELIX_SETUP.md](HELIX_SETUP.md) |
| Zed | [ZED_SETUP.md](ZED_SETUP.md) |
| Sublime | [SUBLIME_SETUP.md](SUBLIME_SETUP.md) |

## Claim boundary

Coexistence guidance explains observed or configuration-backed overlap. It does not prove
that every third-party combination is supported, bundled, or automatically cleaned up.
Installed acceptance on VS Code exercises one clean observation path and one detected
conflict path; other hosts rely on this document and doctor output unless a separate
receipt lane proves more.
