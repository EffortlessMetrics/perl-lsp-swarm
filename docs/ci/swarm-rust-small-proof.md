# Perl LSP Swarm Rust Small Proof

This repository is the high-volume same-repo PR workspace for
`EffortlessMetrics/perl-lsp`.

The first protected swarm lane is `Perl LSP Rust Small Result`. Branch
protection must require that normalized result, not the conditional
implementation jobs for CX53, CX43, or GitHub-hosted fallback.

The router may return `scoped_noop` only when the exact pull-request file set is
the audited narrative path `docs/project/status/release.md`. It reads the
classifier and policy from the event's exact base SHA, reads the PR identity
before and after the paginated files API, and requires the API count to equal
the event's `changed_files`. Base and head repository identities must also be
present and match across the event and both API observations, including a
fork's distinct head repository. Missing bootstrap files, stale identity, API
or decode errors, renames, malformed or duplicate paths, mixed changes, forks
with incomplete evidence, and every unknown path fail closed to full Rust proof.

`Perl LSP Rust Small Result` remains present for a scoped no-op. It succeeds
only when the router supplies the typed subject, base/head SHAs, file count and
digest, symbolic policy/classifier identities, SHA-256 digests of both
base-owned artifacts, and all four implementation/fallback jobs are skipped.
The path digest is SHA-256 over the UTF-8 bytes of the sorted path list encoded
as compact JSON (`["docs/project/status/release.md"]` hashes to
`794d5f956c9b3140e585d22c2d57e2d858bf571128598e641b39ab72e17d23ad`).
The aggregate requires that exact digest and rejects empty-file and all-zero
sentinels for either trusted base artifact.

Initial proof captured:

- same-repo PR fallback route: `26146166886`;
- forced CX43 backfill route: `26146635076`;
- forced CX53 primary route: `26147069092`.

Release, publish, signing, extension, and secrets-heavy workflows remain owned
by the source repository until a separate deliberate migration.
