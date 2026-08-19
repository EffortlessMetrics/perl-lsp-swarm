# A Perl extension for Zed

## Installation

Rust must be installed via [rustup](https://rustup.rs) for development
extensions to work.

Clone the repository, then open Zed's extensions page and click **Install Dev
Extension**. Select the cloned directory and Zed will build and load the
extension automatically.

Currently, this is a work in progress.

The grammar is set up with the
[tree-sitter parser](https://github.com/tree-sitter-perl/tree-sitter-perl);
queries are constantly being improved.

## Language-server identities

The extension exposes three separate products:

| Server ID | Product |
| --- | --- |
| `perlnavigator-server` | Perl Navigator |
| `perl-lsp` | `tree-sitter-perl/perl-tree-sitter-lsp` |
| `perllsp` | `EffortlessMetrics/perl-lsp` |

These IDs are not aliases. Selecting one server never launches another product
behind its identity.

## Using Perl Navigator

Until the next release of Perl Navigator, install it from npm or GitHub releases
and make it available on your `PATH` for Zed to find.

To pass settings, use the following shape and consult Perl Navigator for its
configuration options:

```json
{
  "lsp": {
    "perlnavigator-server": {
      "settings": {
        "perlnavigator": {
          "includePaths": [
            "local/lib/perl5",
            "lib"
          ]
        }
      }
    }
  }
}
```

## Using tree-sitter-perl `perl-lsp`

[`perl-lsp`](https://github.com/tree-sitter-perl/perl-tree-sitter-lsp) is an
opt-in alternative language server built on this grammar. It stays independent
from EffortlessMetrics `perllsp`.

Install its binary on `PATH`, or enable its existing managed-download setting:

```json
{
  "lsp": {
    "perl-lsp": {
      "settings": {
        "download": true
      }
    }
  }
}
```

Select it over Perl Navigator with:

```json
{
  "languages": {
    "Perl": {
      "language_servers": [
        "perl-lsp",
        "!perlnavigator-server",
        "!perllsp",
        "..."
      ]
    }
  }
}
```

## Using EffortlessMetrics `perllsp`

[`perllsp`](https://github.com/EffortlessMetrics/perl-lsp) is a separate native
Rust language server. Select its dedicated server ID and disable the other Perl
servers:

```json
{
  "languages": {
    "Perl": {
      "language_servers": [
        "perllsp",
        "!perlnavigator-server",
        "!perl-lsp",
        "..."
      ]
    }
  }
}
```

The extension first uses a `perllsp` binary already available on the worktree
`PATH`. Otherwise it downloads the latest stable release from
`EffortlessMetrics/perl-lsp` for checked macOS, Linux, and Windows x86-64
targets. It launches the server explicitly as `perllsp --stdio`.

To override the executable while retaining the correct server identity:

```json
{
  "lsp": {
    "perllsp": {
      "binary": {
        "path": "/absolute/path/to/perllsp",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

Use `.perl-lsp.toml` in the project root for configuration shared across
editors. Zed forwards `lsp.perllsp.settings` through
`workspace/configuration` when editor-specific settings are needed.

Managed Windows ARM64 installation is intentionally not claimed yet. A proven
compatible binary already on `PATH` can still be selected explicitly.

## File activation

Perl activation covers `.pl`, `.PL`, `.pm`, `.t`, `.psgi`, `.cgi`, `.fcgi`, and
Perl shebangs. `.pod` remains assigned to the separate POD language and grammar.

## Semantic tokens

The extension includes default mappings for `perllsp`'s custom `sql_string`,
`sql_heredoc_keyword`, and `json_heredoc_key` token types. Zed semantic tokens
remain opt-in through the editor's `semantic_tokens` setting.

## Text objects

In Zed's Vim mode, the extension provides these tree-sitter text objects:

| Object | Around (`a`) | Inside (`i`) | Matches |
| --- | --- | --- | --- |
| function | `af` | `if` | named/anonymous `sub`s and `method`s |
| class | `ac` | `ic` | block-form `package` and `class` declarations |
| comment | `gc` | — | adjacent comments as a group |

## Evidence boundary

Extension compilation proves package integrity, not actual Zed behavior. Each
server, platform, extension version, and binary version needs its own host
receipt before another project should publish an unqualified support claim.
