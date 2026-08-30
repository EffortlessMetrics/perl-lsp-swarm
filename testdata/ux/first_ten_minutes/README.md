# First-ten-minutes representative fixture set

Issue: #5902
Protocol: `docs/releases/v0.18-first-ten-minutes.md`

One content-addressed Perl project per experience family. The complete
v0.18 study uses exactly this finite set; each observation receipt binds
one fixture by `fixture_id`, `family`, and `content_sha256`.

| Fixture | Family | Experience exercised |
| --- | --- | --- |
| `conventional-modules-v1` | `conventional_modules` | multi-file `lib/` packages, imports, bin entry, tests |
| `test-heavy-v1` | `test_heavy` | Test::More discovery, subtests, navigation from tests |
| `framework-shaped-v1` | `framework_shaped` | bounded Mojolicious::Lite-style routing app |
| `environment-sensitive-v1` | `environment_sensitive` | `use lib`/@INC layout, cpanfile, environment introspection |
| `dynamic-boundary-v1` | `dynamic_boundary_control` | AUTOLOAD, symbolic dispatch, string eval fallback |

## Content identity

`manifest.json` records each fixture's `content_sha256`, computed over
all files in the fixture directory:

1. collect regular files recursively;
2. sort by relative POSIX path;
3. feed, per file: path bytes, LF, decimal byte length, LF, file bytes;
4. SHA-256 of the concatenation.

The `first_ten_minutes` xtask example verifies the set and rejects:

- byte drift in any fixture without a manifest refresh;
- a missing or extra family;
- duplicate fixture ids or unsafe fixture paths;
- a checked-in receipt whose `project` does not bind a real fixture.

```bash
cargo run -p xtask --locked --example first_ten_minutes -- \
  --verify-fixture-set testdata/ux/first_ten_minutes
```

Editing any fixture byte requires refreshing `manifest.json` and every
receipt under `fixtures/experience/first_ten_minutes/` that binds it.
