# Context: #1865 — fix(hover): POD documentation cache not invalidated when module file changes externally

## Problem

The POD documentation cache in `format_pod_for_hover()` is keyed only by filesystem path with no modification-time check. When a module file is edited outside the LSP server (e.g., by a build system, external editor, or file sync tool), the hover handler continues returning the stale cached POD documentation instead of re-reading the updated file. This is a correctness issue for users who work in multi-tool environments where Perl modules may be regenerated or modified outside the LSP editor.

The only current cache invalidation mechanism is the capacity-prune policy (drains to half capacity when reaching 1024 entries), which means a file modified externally may remain cached indefinitely if fewer than 512 new modules are hovered in the session.

## Why this approach

The issue's suggested approach is sound: store `(SystemTime, PodDoc)` tuples in the cache and validate mtime on cache hit. This is:

1. **Low-cost**: mtime check only happens on cache hits (not misses), and the lock is already acquired
2. **Graceful**: Falls back to cached POD if file cannot be read (deleted, permission denied)
3. **Simple**: Minimal code changes localized to two functions (cache hit and cache insertion)
4. **Correct**: Matches the existing file-watcher contract expectation that caches are invalidated when files change

The issue explicitly considered an alternative (document POD caching as "best-effort" and accept no invalidation), but correctly rejected it because users expect external edits to be reflected in hover, especially in multi-tool workflows.

## Alternatives rejected

- **No invalidation, document as best-effort**: Rejected because users working with build systems or external editors expect their changes to be visible. A "best-effort" cache would be a silent correctness loss.
- **Use file-watcher integration**: The LSP file-watcher detects external changes to open documents, but the POD cache is path-keyed (not document-keyed) and accumulates entries for every module hovered, most of which are never opened in the editor. The file-watcher only tracks open documents, so a module hovered but never opened would not trigger a file-watch event. Per-hit mtime check is more correct.
- **Add TTL-based expiration**: Simpler than mtime checking but less precise. If a user hovers a module, waits 10 minutes, hovers again, and the module was modified in minute 5, a 5-minute TTL would still return stale data. mtime checking is more deterministic.
- **Use inode number + mtime**: Some filesystems (e.g., Windows NTFS) have low-resolution mtimes (1-second granularity). Inode numbers are not stable across edits on all filesystems. SystemTime is the standard and reliable choice.

## Prior art / duplicates

No existing per-file mtime cache exists in perl-lsp-rs that we can reuse. The semantic_analyzer_cache uses content hashing (`(uri, content_hash)`) because it caches per-document-version (keyed by normalized URI and the hash of the document text). The POD cache is path-keyed (not document-keyed), so content hashing is not applicable.

Prior-art scan: Perl's own module caching (`%INC`, per-interpreter package cache) uses mtime-based invalidation for compile-time reloads. Rust's module ecosystem (e.g., cargo) also uses filesystem mtime for dependency tracking. mtime-based cache invalidation is a well-established pattern.

This is the first (and only) place in perl-lsp-rs that caches POD documentation. The implementation is canonical and introduces no dup-risk.

## Links

- Issue: #1865
- Subsystem: LSP (`crates/perl-lsp-rs/src/runtime/language/hover.rs`)
- Hazard defaults: `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md` — LSP-1 through LSP-4
- Concepts: File modification detection / cache invalidation (portable pattern, also relevant to file-watcher integration)
- Related incident: file-watcher integration expects all downstream caches to be invalidated on external changes; this PR fulfills that contract for the POD cache
- File-watcher contract: CLAUDE.md "Workspace file-watcher contract"

## Implementation notes

- `SystemTime::eq()` is safe for filesystem mtimes because `SystemTime` derives `PartialEq` and compares the underlying time value
- Fallback behavior: if `metadata()` fails, return cached POD (graceful degradation)
- No new dependencies; `std::time::SystemTime` is in std
- Threads: The cache is behind `Arc<Mutex<...>>`, so mtime reads are serialized through the lock (safe)
- Tests will need to use `std::fs::write()` to modify files and potentially `std::thread::sleep()` to ensure mtime granularity (some filesystems have 1-second resolution)
