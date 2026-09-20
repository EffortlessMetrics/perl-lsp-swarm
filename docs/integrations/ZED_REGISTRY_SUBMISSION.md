# Official Zed Perl registry update packet

> **State:** blocked pending the merged upstream extension commit.
>
> **Owner:** [#7910](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7910)

The public Zed registry already contains one Perl extension:

```text
extension ID: perl
registry path: extensions/perl
upstream: https://github.com/tree-sitter-perl/zed-perl.git
current registry version: 0.4.0
current submodule commit: eb27a19e69fed8a041b706b23a1f42fbafb29fd8
```

The future registry submission updates that existing entry. It must not create a
new extension ID, point at the swarm candidate, or publish an unmerged fork
commit.

## Captured registry subject

| Field | Value |
| --- | --- |
| Repository | `zed-industries/extensions` |
| Base commit | `3823ee669031bb22e2d1b8e1bdb1417823808e9a` |
| Tree | `9abdbbddd5ab0a1be93cd4f85155b424409ff8cc` |
| `extensions.toml` blob | `b11c7fe3e57646a9fb9ec243085f362e220df331` |
| `.gitmodules` blob | `d020322a746febba0c8cb9e97183ab13928b860f` |

These values are a snapshot for packet construction. The registry base must be
refreshed after the extension merges and immediately before manual submission.

## Allowed final diff

```text
extensions/perl   move the existing submodule to the exact merged upstream commit
extensions.toml   update [perl].version to the identical extension manifest version
```

The new submodule commit must be reachable from a branch in
`tree-sitter-perl/zed-perl`. The version in the registry and `extension.toml`
must match exactly. The existing HTTPS remote and MIT license remain required.

## Validation

The final packet must retain green results for:

```text
pnpm sort-extensions
current package validation
current danger/repository validation
submodule branch reachability
manifest/registry version equality
exact diff digest
```

It must also record the resolved state of #7908 so publication does not knowingly
create a clean-profile interval in which every Perl provider attempts startup.

## Evidence boundary

A registry PR publishes metadata and a submodule. It does not prove a clean
registry installation or runtime behavior. #7912 remains the only lane allowed
to install the official extension in a fresh profile and promote exact public
Zed support cells.
