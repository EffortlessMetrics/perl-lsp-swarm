# Zed upstream submission packet

> **State:** submission-ready candidate; actual Zed support remains not proven.
>
> **Owner:** [#7759](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7759)

This packet prepares the two upstream changes required for a clean `perllsp`
route in Zed without conflating it with the existing `perl-lsp` server.
Nothing in this packet has been submitted upstream by the repository.

## Exact source identities

| Subject | Identity |
| --- | --- |
| Perl extension repository | `tree-sitter-perl/zed-perl` |
| Prepared base commit | `eb27a19e69fed8a041b706b23a1f42fbafb29fd8` |
| Effortless server ID | `perllsp` |
| Effortless binary | `perllsp --stdio` |
| Effortless release authority | `EffortlessMetrics/perl-lsp` |
| Existing independent server | `perl-lsp` → `tree-sitter-perl/perl-tree-sitter-lsp` |

The staged extension is under
`.ci/fixtures/zed-perl-upstream/zed-perl/`. It preserves Perl Navigator and the
tree-sitter-perl server, adds a third `perllsp` identity, and rejects unknown
server IDs instead of falling through to Perl Navigator.

The fixture-only `Cargo.toml` adds an `rlib` target so host-side unit tests can
exercise the pure dispatch and release-selection helpers. The apply script does
not copy that manifest; the upstream checkout keeps its existing package and
lockfile.

## Apply to an exact upstream checkout

Start from a clean checkout at the prepared base, then run:

```bash
scripts/apply-zed-perl-upstream.sh /path/to/zed-perl
```

The script refuses a dirty checkout or a different base commit. After copying
the candidate, it runs `git diff --check` and prints the verification commands.
When upstream has moved, rebase the candidate deliberately rather than bypassing
the identity check.

## Local verification

From this repository:

```bash
bash scripts/check-zed-upstream-candidate.sh
cargo fmt --manifest-path .ci/fixtures/zed-perl-upstream/zed-perl/Cargo.toml -- --check
cargo clippy --manifest-path .ci/fixtures/zed-perl-upstream/zed-perl/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path .ci/fixtures/zed-perl-upstream/zed-perl/Cargo.toml
cargo build --manifest-path .ci/fixtures/zed-perl-upstream/zed-perl/Cargo.toml \
  --target wasm32-wasip2 --release
cargo test -p xtask --test zed_support_claims --locked
```

The candidate intentionally supports managed downloads only for targets already
present in the checked release contract:

```text
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
x86_64-apple-darwin
aarch64-apple-darwin
x86_64-pc-windows-msvc
```

Windows ARM64 managed installation remains explicit `not_proven`; a binary on
`PATH` is still eligible for deliberate testing.

## Copy-ready Perl extension PR

**Title**

```text
feat: add EffortlessMetrics perllsp as a separate Perl server
```

**Body**

```markdown
## Summary

Add `perllsp` (`EffortlessMetrics/perl-lsp`) as a third, separately identified
Perl language server alongside Perl Navigator and tree-sitter-perl's existing
`perl-lsp` server.

## Product identity

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp
```

The dispatcher is now exhaustive. Unknown IDs return an error instead of
silently launching Perl Navigator.

## Installation and launch

- Prefer a user-installed `perllsp` on the worktree `PATH`.
- Otherwise download the latest stable versioned asset from
  `EffortlessMetrics/perl-lsp`.
- Launch explicitly with `--stdio`.
- Preserve the current archive-layout distinction: nested Unix tarballs and a
  root-level Windows executable.
- Keep Windows ARM64 managed installation unclaimed until a public artifact and
  host receipt exist.

## Editor integration

- Add `.PL`, `.psgi`, `.cgi`, and `.fcgi` activation while keeping `.pod` under
  the existing separate POD language.
- Add semantic-token defaults for the custom SQL/JSON heredoc token types.
- Document selection using the dedicated `perllsp` ID; do not repoint the
  existing `perl-lsp` ID.

## Verification

- Host unit tests cover server-ID dispatch, version normalization, target
  selection, asset names, and extraction paths.
- `cargo build --target wasm32-wasip2 --release`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`

A separate narrow Zed-defaults change keeps both alternative Perl servers
disabled until the user selects one, avoiding unsolicited startup failures or
multiple competing servers.
```

## Required Zed defaults follow-up

The extension API cannot declare an alternative server disabled by default.
The prepared fragment is:

```json
{
  "languages": {
    "Perl": {
      "language_servers": [
        "perlnavigator-server",
        "!perl-lsp",
        "!perllsp",
        "..."
      ]
    }
  }
}
```

Its job is narrow: preserve Perl Navigator as the default while preventing both
alternative servers from starting until selected. It is a separate Zed-core
configuration change, not hidden extension behavior.

## Promotion after upstreaming

Upstream merge is not behavior proof. The support row can move only through:

```text
exact-source development-extension receipt
→ released extension + public perllsp artifact receipt
→ registry-backed documentation promotion
```

The receipt must bind Zed version, extension version/ref, `perllsp` version and
hash, platform/architecture, winning server identity, diagnostics, navigation,
edit/re-query, workspace edit, and clean shutdown.
