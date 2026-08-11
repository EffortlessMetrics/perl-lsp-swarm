# LSP client support evidence

> Generated from `policy/lsp-client-support.toml`. Setup prose, synthetic client profiles, actual clients, and packaged products are different evidence classes.

| Client | Integration mode | Earned tier | Claim boundary |
| --- | --- | --- | --- |
| VS Code | `managed_extension_or_stdio` | `configuration_documented` | The extension and stdio setup are documented. Packaged product proof remains owned by #6056. |
| Cursor | `vscode_compatible_host` | `configuration_documented` | Shared extension protocol facts do not prove Cursor-specific installation, activation, or update behavior. |
| Trae (ByteDance) | `vscode_compatible_host` | `configuration_documented` | Shared extension protocol facts do not prove Trae-specific installation or activation. |
| IntelliJ IDEA / JetBrains IDEs | `lsp4ij_plugin` | `configuration_documented` | The repository proves an LSP4IJ-shaped inline-completion profile, not an actual IntelliJ/LSP4IJ launch or the complete client journey. |
| Neovim | `generic_stdio_client` | `configuration_documented` | The current UX trace is a hand-authored Neovim-shaped capability profile; it does not launch Neovim. |
| Vim | `vim_lsp_plugin` | `configuration_documented` | Configuration is documented; no current actual Vim/vim-lsp receipt is registered. |
| coc.nvim | `coc_language_server` | `configuration_documented` | Configuration is documented; no current actual coc.nvim receipt is registered. |
| Emacs | `eglot_or_lsp_mode` | `configuration_documented` | Configuration is documented; no current actual Emacs client receipt is registered. |
| Helix | `generic_stdio_client` | `configuration_documented` | Configuration is documented; no current actual Helix receipt is registered. |
| Zed | `extension_registered_language_server` | `bridge_or_plugin_dependency` | Zed settings cannot register an arbitrary server alone; direct support is unproven until a perllsp-capable extension and journey exist. |
| Sublime Text | `sublime_lsp_plugin` | `configuration_documented` | Configuration is documented; no current actual Sublime LSP receipt is registered. |
| Amazon Kiro | `vscode_compatible_or_custom_lsp_host` | `configuration_documented` | Configuration is documented; host-specific activation and provider behavior remain unproven. |
| Claude Code | `plugin_lsp_bridge` | `bridge_or_plugin_dependency` | Support depends on an external plugin registration surface; perllsp remains an LSP server, not an agent tool protocol. |
| Codex CLI | `lsp_to_mcp_bridge` | `bridge_or_plugin_dependency` | Codex CLI consumes MCP tools; registering perllsp --stdio directly as MCP is unsupported. |
| Codex Desktop | `custom_stdio_server` | `configuration_documented` | Configuration is documented; no current actual Codex Desktop receipt is registered. |
| OpenCode | `custom_stdio_server` | `configuration_documented` | The workaround is documented and implemented, but no protocol-profile or actual OpenCode journey is registered. |

No row is currently promoted to `packaged_product_proven` or `real_generic_client_proven`. Promotion requires typed actual-client or packaged-product evidence under issue #6739.
