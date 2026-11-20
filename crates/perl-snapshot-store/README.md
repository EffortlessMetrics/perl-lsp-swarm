# perl-snapshot-store

Storage-neutral [`SnapshotStore`](src/port.rs) port and a deterministic
directory-backed reference implementation for versioned restart snapshots
(#4911).

The port validates storage integrity only (framing, digests, manifest
closure, atomic publication). Semantic compatibility and currentness remain
upstream responsibilities (#7452 / #7454).

## Layout

```text
<cache-root>/
  objects/<content-digest>
  manifests/<manifest-identity>
  current/<workspace-profile-key>
  quarantine/<event-id>/
  metadata/store-schema
```

## Proof

```bash
cargo test -p perl-snapshot-store --all-targets --locked
```

Backend-neutral differential fixtures live in `tests/backend_differential.rs`
and are reused from `perl-workspace` integration tests.
