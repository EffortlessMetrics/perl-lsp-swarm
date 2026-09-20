# Use a locally built `perllsp` in this repository

The checked-in `.vscode/settings.json` is intentionally product-neutral. Opening
`perl-lsp-swarm` must not select a development binary, disable managed download,
or point the extension at a private download service.

## Why no repository file can select the binary

The extension declares these settings with `scope: "machine"` in
`vscode-extension/package.json`:

<!-- machine-scoped-keys:start -->
- `perl-lsp.serverPath`
- `perl-lsp.autoDownload`
- `perl-lsp.downloadBaseUrl`
- `perl-lsp.versionTag`
- `perl-lsp.channel`
<!-- machine-scoped-keys:end -->

Other settings are machine-scoped too; these are the ones that decide which
binary runs and where it comes from.

VS Code reads machine-scoped settings **only** from User or Machine settings. A
value placed in `.vscode/settings.json`, in a folder settings file, or in the
`settings` block of a `.code-workspace` file is ignored — `getConfiguration`
never surfaces it. Such a file looks configured and does nothing.

This is deliberate, not an oversight. See `.jules/sentinel.md`, entry
`2026-01-29 - Workspace Configuration RCE`: before machine scope was applied, a
hostile repository could execute an arbitrary binary or redirect the download
simply by being opened. A repository must not be able to choose the executable
the extension runs — including this repository.

So a local build is selected in your own User Settings, and nothing about that
choice is committed.

## Record the candidate

Before pointing the extension at a build, record three facts:

- the checkout SHA the binary was built from;
- the absolute path to the binary;
- the version the binary reports from `perllsp --version`.

These identify a development candidate. They do not make it a public release,
and a candidate's behavior is not evidence about the published extension.

## Select the local build

Run **Preferences: Open User Settings (JSON)** from the Command Palette and add:

<!-- user-settings-example:start -->
```json
{
  "perl-lsp.serverPath": "__REPLACE_WITH_ABSOLUTE_PERLLSP_PATH__",
  "perl-lsp.autoDownload": false
}
```
<!-- user-settings-example:end -->

Replace the placeholder with the absolute path to the exact binary you recorded,
then reload the window. A relative path or a missing file is a local
misconfiguration: fix the path rather than letting another `perllsp` on `PATH`
stand in for the candidate you meant to test.

Because User Settings are not per-folder, this selection applies to every window
until you remove it. Reset before evaluating the installed product.

## Return to normal extension behavior

Remove both keys from User Settings JSON and reload the window. Because the
checked-in repository settings never set `serverPath` or `autoDownload`, the
extension returns to its normal installed-product lifecycle: managed download,
release channel, and version selection all resume.

## Do not

- Do not add machine-scoped keys to `.vscode/settings.json`, to a
  `.code-workspace` file, or to any other committed file. VS Code ignores them
  there, which hides the misconfiguration instead of reporting it.
- Do not put a personal absolute path in shared repository settings.

Task- and launch-level candidate binding belongs to the follow-on repository
workspace train. This file is only the explicit local selection boundary.
