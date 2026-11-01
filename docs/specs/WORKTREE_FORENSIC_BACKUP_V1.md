# Worktree forensic backup v1

## Scope and authority

This is the contract for `cargo xtask worktree-recovery backup`, introduced for
[#12989](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/12989).
It consumes the landed explicit-candidate identity types from
[worktree forensic evidence v2](WORKTREE_FORENSIC_EVIDENCE_V2.md). It does not
change classification, restore a worktree, synthesize `.git/worktrees/**`,
reconstruct an index, or apply recovery.

Path-safety, stable-read, and directory-race checks are the same authority used
by the evidence observer. Backup must not re-derive a weaker symlink, reparse,
or sampled-interval rule.

## Production seam

```text
cargo xtask worktree-recovery backup
  --repository <repo>
  --candidate <path>
  --backup-dir <explicit-output>
  [--json]
```

`--backup-dir` is required and is the only permitted write location. The parent
of that directory must already exist; the command refuses to create intermediate
paths outside the destination. A relative destination with no slash (`backup`)
uses the current directory as that parent. The destination must not overlap the
selected repository, common directory, or candidate. Existing destinations must
be empty regular directories. Any symlink, junction, or reparse component in the
destination path is refused. After the named directory is created or admitted
and canonicalized, backup revalidates it with `symlink_metadata` so a swapped
symlink is refused. Residual races after that pin remain sampled-interval, the
same class as the evidence observer.

## Capture set

A verified backup captures, when present as regular non-reparse files:

- the candidate `.git` pointer bytes, even when the pointer does not parse;
- candidate source-like files (`pl`/`pm`/`pod`/`t`/`plx`/`xs`), including ignored
  source, without following `.git` or unselected sibling worktrees;
- surviving administrative files under the parsed administrative path, and only
  when that path is inside the selected common-dir `worktrees` namespace (the
  same `is_in_admin_namespace` gate as the evidence observer). A forged or
  outside pointer still captures pointer bytes; host files outside that
  namespace are not copied;
- `HEAD`, `packed-refs`, and `refs/**` from the common directory;
- `logs/**` reflogs from the common directory;
- common `.git/config`.

Absent subjects are recorded as `missing`. A symlink, reparse, race, or bound
overflow during capture refuses a verified receipt rather than following an
escape or claiming completeness. Capture uses the shared `TraversalLimits`
surface, including `max_directories`. A file that lands exactly on `max_bytes`
is admitted; later skipped entries do not refuse the backup.

Captured bytes are stored as content-addressed `objects/<sha256>` files. The
receipt lists role, logical path, source path, size, and digest.

## Receipt and verification

`schema_version` is `worktree_forensic_backup.v1`. The decoder requires exact
equality with that label. `verification_digest` hashes the serialized receipt
with `created_at` and `verification_digest` cleared. Timestamp changes do not
change identity; material evidence changes do.

`verify` re-reads the receipt and every named object through the stable-read
path and refuses a digest, size, or schema mismatch. Object file names are
admitted only as lowercase SHA-256 hex (64 characters) before they are joined
under `objects/`; `..`, separators, and non-hex names are refused.

## Retention and restore instructions

Retention is operator-held. The backup writer never deletes a backup and never
sets `auto_delete` to true.

Restore instructions are documentation in the receipt. This command does not
restore. Instructions must prohibit `git worktree add --force`, reset, checkout,
stash, clean, administrative synthesis, and index reconstruction.

## Proof boundary

Focused proof:

```sh
cargo test -p xtask --lib --locked worktree_forensic_backup
cargo test -p xtask --lib --locked worktree_forensic_recovery
cargo test -p xtask --test worktree_forensic_backup --locked
cargo clippy -p xtask --lib --locked -- -D warnings
cargo fmt -p xtask -- --check
```

This contract proves destination admission, content-addressed capture,
self-verification, retention semantics, restore-instruction text, and
before/after byte identity of the selected repository, candidate, refs, config,
and administrative files on the host that ran the fixtures.

It does not prove restore, apply, native Windows reparse behavior unless that
host ran, or that a later `plan --backup-dir` production wire exists (#12991).
