# Zed upstream submission packet

> **State:** blocked pending fan-in; not ready for upstream submission.
>
> **Owner:** [#7909](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7909)
>
> **Programme:** [#7759](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7759)

This repository is preparing a small update to the existing
`tree-sitter-perl/zed-perl` extension. The maintainer will submit it manually
only after the extension, settings, managed assets, Zed defaults, and actual-host
receipts converge on one exact external subject.

## Product identities

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
perllsp              -> EffortlessMetrics/perl-lsp
```

The IDs are not aliases. The proposed `perllsp` route must start exact
`perllsp --stdio`; unknown server IDs must fail instead of falling through to
another provider.

## Current captured subject

| Subject | Captured identity |
| --- | --- |
| Extension repository | `tree-sitter-perl/zed-perl` |
| Base commit | `eb27a19e69fed8a041b706b23a1f42fbafb29fd8` |
| Public extension version | `0.4.0` |
| Extension API | `0.7.0` |
| Candidate source | `.ci/fixtures/zed-perl-upstream/zed-perl/` |

These identities are a working basis, not a permanent submission target.
#7909 must refresh the upstream branch, version, API, grammar references, and
license immediately before packet freeze. A patch that applies with fuzz is not
current.

## Fan-in

The machine-readable packet lives under:

```text
.ci/fixtures/zed-perl-upstream/submission/
```

It cannot become ready until all of these lanes are bound:

| Lane | Required result |
| --- | --- |
| #7898 | Current documentation and support projections are truthful. |
| #7901 | Three-server extension candidate is formatted, linted, tested, and builds for `wasm32-wasip2`. |
| #7902 | Canonical `settings.perl` route and actual Zed configuration behavior are recorded. |
| #7903 | Every included managed target has exact executable public-asset evidence. |
| #7908 | Zed dormant-default compatibility and safe submission order are resolved. |
| #7907 | One current-stable actual Zed host completes the bounded exact-source journey. |

The packet checker refuses `ready` while any digest, subject, result, version,
or submission-order field is absent. The staged PR body also contains explicit
`[BLOCKED: ...]` markers that must all be replaced before manual submission.

## Candidate application

The current extension candidate can be applied to its exact captured base with:

```bash
bash scripts/apply-zed-perl-upstream.sh /path/to/zed-perl
```

The script refuses a dirty checkout or a different commit. The final packet will
also bind the candidate commit, changed-file map, patch digest, and exact green
verification commands.

## Intended upstream diff

The reviewed candidate currently expects only:

```text
extension.toml
src/perl.rs
languages/perl/config.toml
languages/perl/semantic_token_rules.json
README.md
```

It preserves the existing package manifest, lockfile, license, POD language,
grammars, and Tree-sitter queries. Any final added or removed file invalidates
the changed-file map and requires deliberate review.

## Companion Zed change

A separate `zed-industries/zed` packet under #7908 keeps the current default
provider enabled and both alternatives dormant:

```jsonc
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

The actual-host compatibility matrix determines whether that defaults change
may merge first, the extension must merge first, or publication must be
coordinated. The packet currently records that order as unresolved.

## Evidence boundary

A green source build may make the extension reviewable. It does not prove the
public Zed registry route. After manual upstream and registry publication,
#7912 must install through a clean official-registry profile and promote only
the exact host, platform, extension, binary, activation, configuration, and
method cells earned by that run.
