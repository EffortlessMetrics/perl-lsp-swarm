# Product direction

Use root `AGENTS.md` for the current product direction and operating contract.
perl-lsp develops a native Rust Perl parser and a compiler-backed toolchain with LSP
and DAP interfaces. Source identity, semantic confidence, fallback behavior, and dynamic
boundaries must remain explicit.

Do not derive current release status, capability counts, or support promises from this
steering file. Read the relevant current manifests, `features.toml`, owning tests, and
release or installed-artifact evidence. A declared capability is not proof that every
case works. Core analysis and external-tool-backed formatting, linting, or debugging
have different prerequisites; inspect the owning feature before promising runtime-free
behavior.

Keep editor integration identity separate from repository identity. In particular,
the existing Zed `perl-lsp` ID belongs to `tree-sitter-perl/perl-tree-sitter-lsp` and must
not be repointed to this product. Claims about the distinct `perllsp` integration need
current public-registration and actual-host evidence, not a historical planned status.
