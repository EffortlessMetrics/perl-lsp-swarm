# xtask default-test optimization

The workspace sets `profile.test.package.xtask.opt-level = 0` to avoid optimizing
the large xtask test harness during canonical `cargo test` builds. Dependencies
retain their profile settings. Debug assertions, overflow checks, unwind behavior
and `line-tables-only` debugging remain unchanged. Normal development builds and
the custom `agent` profile retain opt-level 1: `agent` inherits `dev`, not `test`.
Release, benchmark and distribution profiles remain unchanged.

This is a package-specific compiler setting, not a cloud capacity guarantee.
Keep storage admission, execution ownership and cancellation guards in place.
Removing the override restores the previous default-test optimization setting.

## Exploratory measurement

A single Linux treatment used source
`71a69a477665e98dd11d1be163a4d08cea51ef64` plus this exact manifest override,
Rust/Cargo 1.95.0, one Cargo job and incremental compilation disabled. Its command
was:

```sh
cargo test --offline --locked -p xtask --bin xtask \
  --test ci_subject --test ci_scope_tests --no-run --message-format=json -vv
```

The command ran through the repository's admitted route with an explicit storage
budget and native descendant ownership. On-disk dependencies were preserved;
clean resident pages from receipt-bound, settled artifacts were released once
before compilation. File-cache recharge remained included in cgroup accounting.
The 16 GiB host retained a 14 GiB cancellation guard, with an earlier 13.5 GiB
operational stop and a 900-second experiment deadline.

The compile completed in 304.29 seconds. Cargo reported 416 artifacts: 381 cached
and 35 fresh, with fresh artifacts exclusively from xtask. The main-bin test
harness was genuinely rebuilt at opt-level 0. Sampled maxima were 11.624 GiB
cgroup memory and 4.345 GiB main-harness rustc RSS; net filesystem allocation grew
by 2,790,637,568 bytes. All 20 selected scope, subject, gate and workflow tests
passed. The original build and runtime owners proved kernel descendant closure;
the build lease was released and OOM counters remained unchanged.

These are sampled maxima, not instantaneous upper bounds. Process ancestry
sampling does not fully attribute aggregate cgroup memory. There was no fresh
control compile, so these results establish neither a speed improvement nor a
causal memory reduction. The source included helper changes absent from the
main-based profile candidate: this measurement is not exact-head qualification
of that candidate, the separate nested-Cargo subject, the custom agent profile,
Windows, or the complete runtime suite. Raw host logs are intentionally excluded.
