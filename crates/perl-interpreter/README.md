# perl-interpreter

Shared Perl interpreter discovery for language-server and debugger consumers.
The crate observes existing filesystem candidates and keeps one process-local
discovery cache. It has no LSP, DAP, parser, workspace, or async-runtime dependency.

## API and preserved behavior

| API | Existing behavior |
| --- | --- |
| `resolve_perl_path_with_toolchain` | Perlbrew, then plenv, then `resolve_perl_path` |
| `resolve_perl_path` | Absolute, nonempty PATH entries, followed by Termux candidates |
| `detect_perlbrew_perl`, `detect_plenv_perl` | Environment-selected toolchain candidates and home-directory fallbacks |
| `find_perl_interpreter` | Explicit configured path without silent fallback; then toolchain discovery, PATH ranking, and OS install locations |
| `find_perl_interpreter_cached` | The same discovery result, invalidated by environment keys and candidate/parent filesystem fingerprints |

The resolver and discovery APIs retain their distinct PATH filtering, ranking,
fallback and result contracts. This extraction does not harmonize them. Existing
file-existence checks do not establish that an interpreter can successfully run.

Discovery does not spawn Perl, grant execution authority, establish workspace
trust, or construct `perl-workspace-core::environment` snapshots. Consumers retain
their launch, authorization and probing responsibilities.

The existing `perl_lsp_rs_core::platform` resolver paths and
`perl_dap::platform` discovery paths re-export this implementation. DAP-specific
path normalization, command formatting and launch-environment assembly remain in
`perl-dap`.

## Focused verification

After repository host setup, run the matching preflight before each compiled
command:

```bash
bash scripts/cargo-admitted --preflight test -p perl-interpreter --locked
bash scripts/cargo-admitted test -p perl-interpreter --locked
```

The integration dependency contract checks production, build, development and
target-specific edges. It also rejects a normal/build dependency path from
`perl-lsp-rs` to `perl-dap`. Cargo graph inspection fails if its instrument is
unavailable. The locked graph query may fetch dependency metadata for other target
platforms; it does not compile those dependencies or require a prewarmed workspace
cache.
