# Use a local `perllsp` in this repository

The checked-in `.vscode/settings.json` is intentionally product-neutral. Opening
`perl-lsp-swarm` must not select a development binary, disable managed download,
or point the extension at a private download service.

Use an explicit local workspace file when the installed extension should run a
binary built from this checkout. The file lives under `.tmp/`, which is already
ignored by Git, and does not change user-level VS Code settings.

## Record the candidate

Before opening VS Code, record the checkout SHA, the exact binary path, and the
binary version. `perllsp --version` reports the binary version. These facts
identify a development candidate; they do not make it a public release.

## Create the inactive local override

Create `.tmp/perl-lsp-swarm.local.code-workspace` with the following content.
The `..` folder path is relative to `.tmp/` and opens the repository root.

<!-- local-workspace-example:start -->
```json
{
  "folders": [
    {
      "path": ".."
    }
  ],
  "settings": {
    "perl-lsp.serverPath": "__REPLACE_WITH_ABSOLUTE_PERLLSP_PATH__",
    "perl-lsp.autoDownload": false
  }
}
```
<!-- local-workspace-example:end -->

Replace the placeholder with the absolute path to the exact binary, then open
this workspace file explicitly. A missing path is a local misconfiguration. Do
not silently substitute another `perllsp` found on `PATH`.

The local file records four distinct facts:

- selected binary path;
- checkout SHA used to build it;
- repository workspace root;
- launch behavior chosen by the extension.

Task- and launch-level candidate binding belongs to the follow-on repository
workspace train. This file is only the explicit local selection boundary.

## Return to normal extension behavior

Close the local workspace, remove
`.tmp/perl-lsp-swarm.local.code-workspace`, and reopen the repository folder.
Because the checked-in settings do not set `serverPath` or `autoDownload`, the
extension returns to its normal installed-product lifecycle.

Do not copy the checked-in `.vscode/settings.json` over itself, commit the local
workspace file, or place a personal path in shared repository settings.
