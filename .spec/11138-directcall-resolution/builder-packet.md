# Builder packet: ASM11138 direct-call resolution assessment

**Issue:** #11138  
**Train node:** ASM11138  
**Schema:** `direct_callable_resolution.v1`

## Slice boundary

Land subject/evidence-axis/assessment types and conservative disposition derivation.
Do not wire live workspace queries or emit diagnostics.

## First falsifier

Can a dynamically dispatched call or method-shaped site be classified as
`exact_unresolved` for a missing direct import? Must be **no**.

## Proof commands

```bash
cargo test -p perl-semantic-facts --all-targets --locked direct_callable
cargo test -p perl-semantic-analyzer --all-targets --locked direct_callable_resolution
cargo clippy -p perl-semantic-facts -p perl-semantic-analyzer --all-targets --locked -- -D warnings
```

## Downstream

#11145 consumes stable assessment fields; #790 consumes exact-unresolved subjects only
after denominator integration lands.
