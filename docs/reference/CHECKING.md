# Checking Perl files

This is the current shipped checking vocabulary. It documents live `perllsp`
behavior, not the planned command split on #10766 / #10672.

## Which check?

```text
Need fast native feedback on listed files?          perllsp --check <files...>
Need a project parser coverage metric?              perllsp --check-project [dir]
Need real-Perl compile observation?                 editor Perl: Check Syntax / DAP (`perl -c`)
```

The hinge is the validator and the claim, not marketing language.
Do not call `perllsp --check` a syntax check without naming the native parser.
Do not describe `--check-project` as a strict all-clean check.

There is no `perllsp --check-project-strict` command on current main.
There is no `perllsp --parsability-report` command on current main.
There is no `perllsp --check-perl` command on current main.

## Native listed-file check: `--check`

```bash
perllsp --check lib/MyModule.pm
perllsp --check lib/MyModule.pm t/basic.t
```

- In-process native parser. It does not execute project Perl, `BEGIN`, `use`,
  or source filters.
- Blocking parser findings fail that file. Exit `1` if any listed path failed
  to read or produced a blocking finding. Exit `0` only when every listed file
  was readable and had no blocking findings.
- Advisories remain visible but non-blocking. An advisory-only file prints
  `path: ok` plus `advisory:` lines and still exits `0`.
- Missing files, unreadable files, and a directory passed to `--check` fail
  that path (exit `1`). For a directory, use `--check-project`.
- This does not claim full Perl compiler or runtime conformance.

## Native project parsability report: `--check-project`

```bash
perllsp --check-project lib/
perllsp --check-project .
```

- Walks `.pm` / `.pl` / `.t` under the directory. Nested `local`, `blib`,
  `vendor`, `node_modules`, `.git`, `target`, `.build`, and `auto` directories
  are skipped unless that skipped name is the explicitly requested root.
- Prints scanned-file counts, a clean-parse percentage, blocking findings,
  advisories, and paths that could not be scanned.
- The live threshold is 80% clean among **scanned** files. Exit `0` at or above
  80%, including when some scanned files still have blocking findings. That is
  not a strict all-clean check. `perllsp --check-project . && echo "All files
  parse clean"` is wrong.
- Advisories remain visible but do not affect the parsability verdict.
- Unreadable paths (symlink loops, permission errors) are listed. Percentages
  cover scanned files only.
- If the walk finds no Perl files, the command prints `No Perl files (.pm, .pl,
  .t) found.` and currently exits `0`. A missing or non-directory path exits `1`.
- This does not claim hermeticity, sandboxing, or universal project correctness.

## Real-Perl compile observation

VS Code **Perl: Check Syntax** runs PATH `perl -c` on the saved active file
(`vscode-extension/src/documentCommands.ts`). DAP pre-launch runs `perl -c`
with the debug adapter's configured interpreter. Either path may execute
compile-phase code (`BEGIN`, imports, source filters). This is optional and is
not a native-language-intelligence prerequisite. It is not sandboxed and is not
a `perllsp` flag.

Do not show project-recursive real-Perl checking, dirty-buffer compile, or
workspace-controlled execution as current `perllsp` commands.

## Examples and exits

| Command | Exit `0` | Exit `1` |
| --- | --- | --- |
| `perllsp --check file.pl` | listed files readable, no blocking findings | any listed path unreadable or blocking |
| `perllsp --check-project dir` | no Perl files, or ≥80% of scanned files clean | missing/non-directory path, or below 80% |
| PATH / configured `perl -c` | Perl accepted the saved file | Perl rejected it |

Stdout for `--check` is `path: ok` or `path: FAIL - …` plus optional context
and advisory lines. Read failures (`missing`, unreadable, directory) print to
**stderr** with a hint and still count toward exit `1`; they do not emit a
`path: FAIL` stdout record. Stdout for `--check-project` is the `Perl Project
Parsability Report`. Neither command currently emits a versioned machine
project-check document.
