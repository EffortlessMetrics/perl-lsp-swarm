# perl-tidy

`perl-tidy-cli` provides the standalone `perl-tidy` executable. It calls the
`perl_tidy` native library directly. Neither the command nor the library selects
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
perl-tidy --write lib/One.pm lib/Two.pm
```

Source/stdout mode accepts one input and emits only the verified native rendered
bytes. A refusal or failure emits no source and returns a nonzero status. Human
diagnostics go to stderr. JSON mode emits one report and never mixes in source,
logs, terminal color, or human status lines. A broken output pipe is an output
failure, including when native formatting succeeded.

Check mode is successful only when every selected input is a legitimate native
`NoChange`. `Applied` means the file would change and exits 1. A refusal is never
reported as clean merely because the formatter retained the source bytes.

Paths are explicit. There is no recursive scan, glob interpretation, ignore
policy, or source-control integration. Existing paths are canonicalized, sorted,
and deduplicated. Input must be UTF-8. File paths must also be representable as
UTF-8 so machine reports retain exact path identity.

The per-input limit is 8 MiB by default; `--max-bytes` can select 1 byte through
64 MiB. A command admits at most 1,024 selected arguments and 64 MiB of total
source. These are CLI admission limits, not a claim of cooperative cancellation
or bounded work at every native pipeline stage.

`--stdin-filename` is a logical report label. The command does not open it, infer
project settings from it, or permit it to authorize `--write`.

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

## Write contract

The current write backend supports Linux regular files with one hard link,
ordinary writable permission bits, matching replacement owner/group, and no
extended attributes or ACL metadata. Unsupported file types, symlink components,
readonly files, special permission bits, unknown metadata, and unsupported
platforms refuse before source mutation. Other platforms can use read-only
stdout/check/JSON modes.

The command reads and formats every input first. Each native result must pass
the library's exact edit-application validation. One refusal, invalid
configuration, read error, resource limit, or failed proof blocks the complete
write plan. No-change rows participate in source revalidation too.

For an admitted plan, the command:

1. Holds advisory exclusive locks on the original files.
2. Revalidates source identity, metadata, and exact bytes.
3. Stages complete replacements and recovery copies in each destination's own
   directory, preserving permissions and checking owner/group.
4. Revalidates the full selection after staging, then each source immediately
   before its replacement.
5. Atomically replaces each file, checks the resulting bytes and metadata, and
   syncs the containing directory.
6. Removes staging/recovery copies after success. On a commit failure, it
   attempts guarded rollback of files already replaced.

Each replacement is atomic. A multi-file batch is **not crash-atomic**. There is
no filesystem-wide transaction or journal. Advisory locks do not constrain
non-cooperating writers, and rechecking a path is not an atomic comparison and
replacement operation. Run write mode with exclusive control of the selected
files and their parent directories.

Rollback restores a file only while its current identity, metadata, and bytes
still match the command's replacement. It never deliberately overwrites a later
writer's detected changes. If restoration cannot be established, the command
returns `recovery_required`, identifies the affected subject, and retains the
original recovery copy. Cleanup failures are also reported. Recovery copies
must not be deleted merely to claim that a failed operation left no temporary
files.

Rollback uses the original copy prepared before any commit; it does not need
another temporary file or another content write. If that copy was successfully
renamed back but final verification or directory synchronization fails, its
destination is listed in `unverified_restore_paths`. That is distinct from a
separate original copy still retained in `recovery_files`.

Replacement preserves the admitted ordinary permissions and owner/group. File
identity and modification/creation metadata can change as part of replacement.
Files with extended metadata are refused rather than silently losing it.
Abrupt process termination or machine failure can leave stages or a partially
committed batch; this implementation makes no crash-recovery or signal-driven
cancellation guarantee.

## Machine output and status

The `perl_tidy_cli.v1` JSON report contains executable/package identity,
invocation mode, effective configuration and field sources, normalized ordered
subjects, canonical native outcomes and diagnostic codes/severity/byte locations, source/rendered digests,
byte counts, edit-application verification, and separate filesystem effects.
It omits diagnostic messages, which can contain source tokens, and contains no
source or rendered text by default. As a **local command report**,
it contains exact selected file/configuration paths; review it before sharing.

`complete` means the whole selected input set reached reporting. It does not
turn a refused or failed row into proof. Early selection/input/configuration
failure has `complete: false`; argument-parser errors and help/version output
are outside the JSON envelope. During write mode the native outcome describes
the computation, while `filesystem_effect` and `transaction` describe whether
its bytes were committed, blocked, restored, or require recovery.

| Exit | Class | Meaning |
| --- | --- | --- |
| 0 | `success` | Requested output delivered, check clean, or admitted write committed. |
| 1 | `would_change` | Check found at least one eligible native change. |
| 2 | `refused` | Native formatter deliberately withheld edits. |
| 3 | `invalid_configuration` | Native configuration could not be admitted. |
| 4 | `resource_limited` | CLI input/count/aggregate admission limit exceeded. |
| 6 | `stale_source` | Source identity, metadata, or bytes changed after planning. |
| 7 | `filesystem_error` | Read, path, metadata policy, staging, replacement, or cleanup failed. |
| 8 | `failed_or_not_proven` | Native result did not establish a safe outcome. |
| 9 | `invalid_input` | Source/path representation is not admitted. |
| 10 | `usage` | Arguments or selection are invalid. |
| 11 | `output_error` | Output could not be delivered, including broken pipes. |
| 12 | `instrument_failure` | Native serialization or exact edit-application proof failed. |
| 13 | `recovery_required` | A failed write could not safely restore every changed subject. |

For multiple native rows, the highest nonzero numeric class determines the
command status. Filesystem and delivery failures can supersede that aggregate.
Numeric status 5 is not assigned. Native cooperative cancellation and additional
resource classes will require their actual library contract before being
reported here; the CLI does not fabricate those outcomes.

## Focused verification

The package's integration tests execute the `perl-tidy` binary directly. They
cover clean/change/refusal check distinctions, clean source/JSON streams,
explicit configuration, input limits, deterministic path selection, and a
working `perltidy`/`perl` on PATH that is never invoked. Linux write tests cover
plan rejection, mode preservation, second-check idempotence, readonly files,
symlinks, hard links, stale no-change members, commit failure, and a rollback
conflict that preserves the later writer and retains original recovery bytes.

Use the repository's Cargo admission route for compiled proof:

```sh
bash scripts/cargo-admitted --preflight test -p perl-tidy-cli --locked
bash scripts/cargo-admitted test -p perl-tidy-cli --locked
```

These commands are the intended focused proof, not a statement that this README
has run them or that an installed release has been qualified.
