# IntelliJ IDEA Legacy Raw Command Setup

Use this fallback for local/unreleased `perllsp` candidates, temporary custom launch flags, or a LSP4IJ build where the relevant Perl template route is unavailable.

For the maintained LSP4IJ integration model, start with [IntelliJ IDEA / LSP4IJ Setup](INTELLIJ_IDEA_SETUP.md).

This page documents **manual configuration**, not an actual-client support verdict.

## Add a raw-command server

1. Open **Settings > Languages & Frameworks > Language Servers**.
2. Add a new server definition.
3. Use the canonical server process:

| Field | Value |
| --- | --- |
| Name | `perl-lsp` |
| Command | `perllsp --stdio` |
| File patterns | `*.pl`, `*.pm`, `*.t` |

The initial file-family boundary is intentionally limited to `.pl`, `.pm`, and `.t`. Do not add `.psgi`, `.cgi`, `.fcgi`, POD, XS, templates, or extensionless scripts merely because another integration maps them or the parser can inspect them. Those are independent support cells.

## Binary identity

If the IDE does not inherit your shell `PATH`, use an absolute path to the intended candidate.

Unix-like shells:

```bash
command -v perllsp
perllsp --version
```

Windows PowerShell:

```powershell
where.exe perllsp
perllsp --version
```

On Windows, a raw command may use a path such as:

```text
C:/path/to/perllsp.exe --stdio
```

When collecting interoperability evidence, record the exact binary path/version/hash rather than accepting any command named `perllsp`.

## Checked descriptor example

The checked descriptor at [`lsp4ij-perl-lsp.json`](lsp4ij-perl-lsp.json) uses the same bounded contract:

```json
{
  "name": "Perl Language Server",
  "languageId": "perl",
  "fileExtensions": ["pl", "pm", "t"],
  "command": ["perllsp", "--stdio"]
}
```

This descriptor is a manual setup example. It is not the canonical upstream LSP4IJ template and must not become a second settings or installer authority.

## Project configuration

Prefer `.perl-lsp.toml` for shared project/repository behavior.

The raw-command route follows the same configuration authority as every other generic client:

```text
.perl-lsp.toml
  portable project/repository configuration

LSP client settings
  sparse user/editor overrides using canonical perl.* keys where supported

initializationOptions
  only values that genuinely require initialize/reinitialize timing
```

Do not copy VS Code `perl-lsp.*` extension settings into this generic LSP route.

## Initialization options

When a setting is genuinely initialization-time, the server-native shape is rooted at `perl`:

```json
{
  "perl": {
    "workspace": {
      "includePaths": ["lib", "vendor/lib"]
    }
  }
}
```

Do not use `initializationOptions` as a blanket replacement for live configuration. Field timing/scope belongs to the canonical configuration/schema authority.

## Verify the manual route

1. Open a `.pl`, `.pm`, or `.t` file.
2. Confirm the LSP4IJ console shows the intended `perllsp --stdio` process.
3. Confirm the exact binary version/path is the candidate you intended to test.
4. Introduce and remove a bounded syntax error to verify the configured route is active.
5. Exercise only the semantic cells you actually intend to claim or troubleshoot.
6. Shut down/restart the server and confirm the old process does not remain orphaned.

A successful launch or diagnostic proves only the cells exercised. It does not make every `perllsp` capability an IntelliJ/LSP4IJ support claim.

## When to leave this route

Return to the normal template path when:

- the released/corrected LSP4IJ template for your cohort is available;
- you no longer need a local/unreleased binary or custom launch flags; and
- the exact template/install subject you want to claim is directly receipted.

Keep manual configuration, locally imported corrected templates, and released built-in templates as separate evidence subjects.
