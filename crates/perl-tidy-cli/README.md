# perl-tidy

`perl-tidy-cli` provides the standalone `perl-tidy` executable for source output
and read-only file checks. It calls the `perl_tidy` native library directly.
Neither the command nor the library selects
Perl::Tidy from PATH, starts Perl, or requires an LSP server.

Build/install from this workspace:

```sh
cargo install --path crates/perl-tidy-cli --locked
```

This is a source-install command, not a claim that this package version has been
published to a registry or included in a release archive.

## Commands

```sh
perl-tidy script.pl
perl-tidy --stdout script.pl
perl-tidy --stdin < script.pl
perl-tidy --stdin --stdin-filename lib/Example.pm < source.txt
perl-tidy --check lib/One.pm lib/Two.pm
perl-tidy --check --json lib/One.pm lib/Two.pm
perl-tidy --config-effective --json script.pl
perl-tidy --explain script.pl
```

The command does not replace input files. `--write` is unsupported and is
rejected during argument parsing, before input files are opened. File
replacement remains a separate delivery under #9312; this package implements
the read-only command rollout from #9306.

Source/stdout mode accepts one input and emits only the verified native rendered
bytes. A refusal or failure emits no source and returns a nonzero status. Human
diagnostics go to stderr. JSON mode emits one report and never mixes in source,
logs, terminal color, or human status lines. A broken output pipe is an output
failure, including when native formatting succeeded.

Check mode is successful only when every selected input is a legitimate native
`NoChange`. `Applied` means the file would change and exits 1. A refusal is never
reported as clean merely because the formatter retained the source bytes.
File checks re-read selected regular files and compare observed metadata and
exact source bytes before reporting. These checks do not lock out concurrent
changes or create an atomic snapshot of a multi-file selection.

Paths are explicit. There is no recursive scan, glob interpretation, ignore
policy, or source-control integration. Existing paths are canonicalized, sorted,
and deduplicated. Symlink inputs resolve to their canonical regular-file
targets. Input must be UTF-8. Source and configuration paths must also be
representable as UTF-8 so machine reports retain exact path identity. An
unrepresentable selected or resolved configuration path is rejected with exit 9.

The per-input limit is 8 MiB by default; `--max-bytes` can select 1 byte through
64 MiB. A command admits at most 1,024 selected arguments and 64 MiB of total
source. These are CLI admission limits, not a claim of cooperative cancellation
or bounded work at every native pipeline stage.

`--stdin-filename` is a logical report label. The command does not open it, infer
project settings from it, or create a file at that path.

## Effective configuration

The configuration authority in this version is the library's
`NativeFormatConfig::default()`, optionally replaced by one complete explicit
JSON configuration, followed by explicit command-line overrides. The command
does not keep private formatter defaults or native rendering logic.

```sh
perl-tidy --config-effective --json script.pl
perl-tidy --config native.json --line-width 100 --indent-width 2 script.pl
```

Save the `configuration.effective` object from the JSON report as `native.json`
to reuse it. The file must contain a complete native configuration object, not
the complete CLI report. Native validation rejects unknown fields, external
engine/process fields, and invalid numeric limits. JSON configuration files are
limited to 1 MiB.

Available overrides are `--line-width`, `--indent-width`, `--tabs`/`--spaces`,
`--final-newline preserve|insert|trim`,
`--brace-placement same-line|next-line`,
`--else-placement cuddled|separate-line`,
`--keyword-spacing space|compact`, and
`--trailing-comma preserve|add-when-wrapped`.

`--config-effective` reports normalized values and the source of each value. It
does not read source, project configuration, or environment-selected profiles.
`--explain` runs the same native formatting operation and presents its canonical
outcome, reason, identity, safety evidence, and next action.

The versioned production profile, per-root effective configuration, and bounded
`.perltidyrc` import are separate library contracts. This command does not claim
those missing policies: JSON reports `production_profile: null` and
`project_resolution: "not_performed"`. No ambient `.perltidyrc`, `PERLTIDY`, or
home-directory profile changes its selected configuration.

## Machine output and status

The `perl_tidy_cli.v1` JSON report contains executable/package identity,
invocation mode, effective configuration and field sources, normalized ordered
subjects, canonical native outcomes and diagnostic codes/severity/byte locations,
source/rendered digests, byte counts, and exact edit-application verification.
It omits diagnostic messages, which can contain source tokens, and contains no
source or rendered text by default. As a **local command report**,
it contains exact selected file/configuration paths; review it before sharing.

`complete` means the whole selected input set reached reporting. It does not
turn a refused or failed row into proof. Early selection/input/configuration
failure has `complete: false`; argument-parser errors and help/version output
are outside the JSON envelope. Reports describe the native computation and
contain no file-replacement or recovery result.

| Exit | Class | Meaning |
| --- | --- | --- |
| 0 | `success` | Requested output delivered or check clean. |
| 1 | `would_change` | Check found at least one eligible native change. |
| 2 | `refused` | Native formatter deliberately withheld edits. |
| 3 | `invalid_configuration` | Native configuration could not be admitted. |
| 4 | `resource_limited` | CLI input/count/aggregate admission limit exceeded. |
| 6 | `stale_source` | Source or observed metadata changed during reading or revalidation. |
| 7 | `filesystem_error` | Source path, regular-file admission, or read failed. |
| 8 | `failed_or_not_proven` | Native result did not establish a safe outcome. |
| 9 | `invalid_input` | Source/path representation is not admitted. |
| 10 | `usage` | Arguments or selection are invalid. |
| 11 | `output_error` | Output could not be delivered, including broken pipes. |
| 12 | `instrument_failure` | Native serialization or exact edit-application proof failed. |

For multiple native rows, the highest nonzero numeric class determines the
command status. Filesystem and delivery failures can supersede that aggregate.
Numeric status 5 is not assigned. Native cooperative cancellation and additional
resource classes will require their actual library contract before being
reported here; the CLI does not fabricate those outcomes.

## Focused verification

The package's integration tests execute the `perl-tidy` binary directly. They
cover clean/change/refusal check distinctions, clean source/JSON streams,
explicit configuration, input limits, deterministic path selection, and a
working `perltidy`/`perl` on PATH that is never invoked, with separate execution
controls for both traps. They also cover unsupported `--write` rejection,
unchanged input bytes, read-only files and symlink alias normalization. Snapshot
tests reject changed content even when observed metadata is preserved, and
reject a symlink substituted after reading.

Use the repository's Cargo admission route for compiled proof:

```sh
bash scripts/cargo-admitted --preflight test -p perl-tidy-cli --locked
bash scripts/cargo-admitted test -p perl-tidy-cli --locked
```

These commands are the intended focused proof, not a statement that this README
has run them or that an installed release has been qualified.
