# `perllsp` nvim-lspconfig submission packet

External target: `neovim/nvim-lspconfig`  
Upstream path: `lsp/perllsp.lua`  
Local behavior authority: `scripts/ux/neovim/perllsp.lua`

The staged file in this directory is preparation evidence only. Do **not** represent it as upstream availability until the external PR is merged and a consumable upstream ref/release contains it.

## Submission gates

Before opening the external PR, record all of:

- #7743 passes for the exact proposed command/filetype/root contract;
- #7124 supplies the deep actual-Neovim lifecycle receipt for the candidate `perllsp`;
- #7716 supplies the supported-floor/current-stable compatibility rows;
- one current nvim-lspconfig new-config admission path is satisfied (normally the upstream project's popularity/adoption criterion or accepted alternative evidence);
- the public `EffortlessMetrics/perl-lsp` README/install route and `perllsp --stdio` identity are current;
- the staged file still passes `check_nvim_lspconfig_candidate.lua` against the canonical local fixture.

## External PR procedure

1. Copy `lsp/perllsp.lua` from this directory to the root `lsp/` directory of a current `neovim/nvim-lspconfig` checkout.
2. Run the upstream lint and documentation-generation commands required by its current `CONTRIBUTING.md`.
3. Open the upstream PR as draft until its local checks and admission evidence are attached.
4. Keep the patch limited to the config and any upstream-required generated/schema files.
5. After merge, record the upstream merge commit and first consumable ref/version in #7722/#7122 before changing our docs to prefer `vim.lsp.enable('perllsp')` without a local config file.

Do not add project-specific `settings`, keymaps, completion setup, virtual-document shims, or other framework behavior to the upstream config. Those are separate client/user concerns.
