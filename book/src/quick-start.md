# Quick Start

Start here to open a Perl file, navigate to a function, and see diagnostics
update after an edit.

## Install for your editor

**VS Code:** install the extension, then open your project folder:

```bash
code --install-extension EffortlessMetrics.perl-lsp-rs
```

The extension can download a server binary for your platform if one is not
already available. Open a `.pl` or `.pm` file and run **Perl: Run Health Check**
from the Command Palette. Confirm the LSP binary check passes, then try the
file below.

**Other editors:** use the
[installation guide](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/INSTALLATION.md#prebuilt-releases)
to choose a release archive for your platform and put `perllsp` on your `PATH`.
The guide also covers Cargo and source builds. Rust is needed only for those
build paths; the native server does not need a Perl runtime for core editing.
Perl is needed to run your programs, tests, and debugger.

Before configuring a manually installed server, check that your shell can find it:

```bash
perllsp --version
perllsp --health
```

`--health` should print `ok <installed-version>`. This checks that the binary
starts; the editor checks below establish that it can answer editing requests.
Follow [Editor Setup](./getting-started/editor-setup.md) to register the server
with your editor, then open your project folder.

## Try your first edit

Create `hello.pl` in the project folder and open it in your configured editor:

```perl
#!/usr/bin/env perl
use strict;
use warnings;

sub greet {
    my ($name) = @_;
    print "Hello, $name!\n";
}

greet("World");
```

1. Use **Go to Definition** on `greet` in the last line. It should jump to
   `sub greet` in this file.
2. Hover over `print` to see its documentation.
3. Delete the closing `}` of `sub greet`. Look for a syntax diagnostic in the
   editor's Problems list or diagnostic display.
4. Restore `}` and save. The syntax diagnostic should clear. Change `World`
   to your name and check that Go to Definition still reaches `sub greet`.

If these checks work, the server is connected and responding to your edits.
Next, explore [completion and other LSP features](./user-guides/lsp-features.md)
or [configuration](./getting-started/configuration.md) for your project.

## If something fails

- **`perllsp` is not found:** follow the installation guide's `PATH` setup.
  A source build creates `target/release/perllsp` (`perllsp.exe` on Windows);
  building alone does not install it on `PATH`. Install the binary or configure
  your editor with its absolute path.
- **The binary starts but the editor has no answers:** check the server path,
  file type, and registration in [Editor Setup](./getting-started/editor-setup.md).
  `perllsp --stdio` is the editor's launch command: it waits for LSP messages,
  so running it in a terminal is not an editing check.
- **Project modules are missing:** run `perllsp --doctor` from the project
  folder for a setup report, then use the
  [troubleshooting guide](./user-guides/troubleshooting.md).

For deeper work, use the [performance guide](./advanced/performance-guide.md),
[debugger guide](./user-guides/debugging.md),
[known limitations](./user-guides/known-limitations.md), or
[contributor guide](./developer/contributing.md).
