# Helix initialize profiles

These fixtures preserve two materially different Helix client subjects for
`perllsp` capability-negotiation tests:

| Fixture | Helix subject | Diagnostics | Relative watcher patterns | Workspace trust |
| --- | --- | --- | --- | --- |
| `25.07.1.initialize.json` | official `25.07.1`, commit `a05c151bb6e8e9c65ec390b0ae2afe7a5efd619b` | push | no | absent |
| `master-079a789e.initialize.json` | commit `079a789e8cb08ead67f19e1971a1b7438b37354b` | pull | yes | present |

The capability objects are source-derived from the exact `InitializeParams`
construction in `helix-lsp/src/client.rs`, with snippets enabled. Only process,
workspace-path, and workspace-folder identities are normalized. The test replaces
those normalized fields with an isolated workspace before replaying each profile
through Cargo's exact `CARGO_BIN_EXE_perllsp` candidate.

This is **protocol-profile evidence**, not an actual-editor receipt. It proves
that the shipped server negotiates the exact checked Helix shapes, including the
stable/master watcher and diagnostic boundaries. It does not prove that `hx`
launched the candidate, polled diagnostics, rendered results, applied edits, or
shut down correctly. Actual-host evidence belongs to #7714 and #7780 and uses
the generic receipt contract from #7777.

## Refresh procedure

For a new Helix subject:

1. Pin the exact tag or commit.
2. Inspect the serialized `InitializeParams` construction at
   `helix-lsp/src/client.rs`.
3. Build that exact subject and capture one real initialize request using an
   isolated `languages.toml` and a bounded LSP capture peer.
4. Normalize only `processId`, `rootPath`, `rootUri`, and `workspaceFolders`.
5. Compare the captured request with the source-derived fixture. Any difference
   is a red finding; do not hand-edit one cohort until it resembles another.
6. Add a new fixture rather than rewriting historical evidence for a later
   release.
7. Run:

   ```bash
   cargo test -p perllsp --test helix_capability_profiles --locked
   ```

A distribution backport is a new subject even when its displayed version matches
an existing release. The fixture metadata must bind the executable/source ref
that actually supplied the capability object.
