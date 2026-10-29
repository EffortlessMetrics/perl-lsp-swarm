# Builtin Cargo Linux lifecycle correction

Owner: #11659, existing process-supervision owner; composition remains #11660.
This focused follow-on is based on exact Clippy route
8a375c220a2f8450c234a5aacf8c27a2bcb82bd6, separately from storage PR #17478.

The consumer receipt at 373f4dac880a1be2d745cc8369b5be1d6d15b194,
`.spec/17480-linux-qualification/receipt.json`, records a builtin-boundary
discriminator: fake Cargo exited 0, its lease was absent, and its detached helper
was still kernel-waitable/live under an outer owner. That owner later proved
closure. Source used the same early-release branch for 101. See
https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11659#issuecomment-6094824141.

The smallest existing-owner correction reuses ClippyTree and call_clippy for
builtin Cargo on Linux. Existing names/default Clippy receipts remain compatible
with consumer tools; an optional operation label supplies Cargo receipts. There
is no new supervisor, command registry, renderer, resource manager or reaper.
Cargo command admission, ordered argv, PATH/toolchain resolution, environment,
private target/build/temp paths, shared lease and capacity arithmetic are unchanged.
The default reserve remains 40 GiB. Missing Linux ownership capabilities refuse
before product launch, without fallback. Preflight remains a read-only snapshot.

The existing exclusive single-thread native CPython scope owns only newly started
setup/product descendants. Original lease identity/marker precedes launch. Every
Linux product status requires kernel ECHILD with __WALL before matching release;
failed closure/release returns 75, settled cancellation returns 130, otherwise the
product status is preserved separately. Same native cancellation, grace, 30-second
post-leader drain, pidfd waitability and failure controls remain in force. Startup,
ownership, signal and output mechanisms are reused, not generalized.

Other platforms keep the legacy builtin behavior explicitly without Linux child
closure. Native Windows remains #17482; macOS and other native hosts are unqualified
by this change. Missing owner-child tables can prevent signaling unknown detached
helpers; deadlines retain the lease. Unrelated pre-existing services and separate
artifact consumers are outside the launched tree. Old attempts cannot be adopted
or retroactively settled, including old consumer 1b03b05. No host scans, unrelated
environment reads, OS/protection changes, old cleanup or storage-branch mutation.

This is a bounded correction of an existing route, not completion of #11659's
canonical typed request/observation, output capture, native Windows or composition
contracts. Rollback restores the known leader-only Linux reuse limitation; do not
present rollback as safe descendant settlement. Any source change returns through
focused lifecycle proof and fresh review before publication.
