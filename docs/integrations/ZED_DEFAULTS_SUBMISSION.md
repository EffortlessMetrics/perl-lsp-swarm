# Zed Perl server-order defaults packet

> **State:** exact-base candidate; actual-host compatibility and submission order remain not proven.
>
> **Owner:** [#7908](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7908)

This packet prepares the independent `zed-industries/zed` change that keeps
alternative Perl language servers dormant until the user selects them. It does
not change either extension's download behavior or make a provider recommendation
beyond preserving the current default.

## Exact subject

| Field | Value |
| --- | --- |
| Repository | `zed-industries/zed` |
| Base commit | `7733b9922665f103abda7c6a3fde6b9dfdc8eba9` |
| Target | `assets/settings/default.json` |
| Target blob | `a03ad8874243f167e86deba8f975268eb384d20f` |

The patch adds one language row:

```jsonc
"Perl": {
  "language_servers": [
    "perlnavigator-server",
    "!perl-lsp",
    "!perllsp",
    "..."
  ],
},
```

`perlnavigator-server` remains the existing enabled default. `perl-lsp` and
`perllsp` are independent alternatives and remain selectable through user
settings. The trailing `"..."` preserves Zed's normal extension/user ordering
semantics.

## Apply

From this repository:

```bash
bash scripts/apply-zed-core-perl-defaults.sh /path/to/zed
```

The script refuses a dirty checkout, a different base commit, or a changed
default-settings blob. It runs `git apply --check` before applying the patch and
finishes with `git diff --check`.

## Required host matrix

The checked matrix at
`.ci/fixtures/zed-perl-upstream/zed-core/compatibility-matrix.v1.json` keeps all
four combinations unproven until exercised:

| Zed defaults | Perl extension | Required observation |
| --- | --- | --- |
| current | public 0.4.0 | capture current behavior |
| candidate | public 0.4.0 | existing `perl-lsp` stays quiet; unknown negated `perllsp` is harmless or order is constrained |
| current | staged three-server extension | known pre-default interval; not publication-ready |
| candidate | staged three-server extension | only Perl Navigator starts by default; alternatives remain selectable |

The actual-host receipt must also prove that selecting either alternative starts
only that exact provider, an explicit two-provider list starts two providers,
and a missing selected server never falls through to another identity.

## Copy-ready PR material

**Title**

```text
settings: keep alternative Perl language servers disabled by default
```

**Body**

```markdown
## Summary

Add a Perl language-server order to the default settings so the current Perl
Navigator provider remains enabled while the two alternative providers stay
dormant until selected:

```text
perlnavigator-server  enabled current default
perl-lsp              disabled alternative from tree-sitter-perl
perllsp               disabled alternative from EffortlessMetrics
```

The trailing `...` is retained so user and extension registrations continue to
participate in the normal ordering rules. Users can still select either
alternative, both alternatives, or a different order in their own settings.

This change contains no binary path, download setting, or provider-specific
runtime configuration. It prevents optional providers from producing startup
failures merely because their extension IDs are declared.
```

## Evidence boundary

The patch and external subject identity are checked. The packet is not ready for
manual submission until #7907 records the compatibility matrix and resolves
whether this change may merge before the extension update or requires a
coordinated order.
