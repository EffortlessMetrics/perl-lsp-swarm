# Rust Small receipt verifier

Internal, non-published workspace entrypoint for verifying a Rust Small receipt
without compiling the unrelated product dependencies of the `xtask` package:

```sh
cargo run --locked -p perl-rust-small-verifier -- --verify-receipt PATH
```

Run from the checkout being verified with the receipt's exact Rust toolchain.
The default command accepts only verification; it never selects product-proof execution.
It retains canonical artifact admission, checkout cleanliness and candidate,
toolchain, step, census, outcome and profile validation. Verification of a
coherent failure receipt retains the canonical verifier's existing behavior;
it does not convert the receipt's product result into success.

## Source ownership

`xtask/src/tasks/rust_small_proof.rs` and its `rust_small_proof/artifact_admission.rs`
submodule remain the sole canonical implementation. This package's small library wrapper
compiles those existing files directly and retains the task runner's intentional
stdout/stderr diagnostic lint policy. There is no copied validator or alternate
receipt contract. The relative source path is internal to this same repository;
this package requires a complete checkout and is explicitly not publishable.
The xtask command and both lightweight binaries consume the same semantic source
and canonical tests.

The governed `.ci/rust-standard-proof.sh` uses the separate, no-argument producer:

```sh
cargo run --locked -p perl-rust-small-verifier --bin perl-rust-small-proof
```

It delegates directly to canonical `run(None, None)` with the default receipt path.
Any argument is rejected before proof execution. Existing xtask producer and verifier
commands remain supported. The package's explicit `default-run` preserves the strict
verification CLI for commands without `--bin`.

This removes the unrelated xtask dependency graph from administrative bootstrap;
product commands still build and execute their full canonical dependency graphs.
No elapsed speedup or 20-minute result is established without composed cold consumer
qualification. Source custody, working directory, toolchain and resource admission
remain governed by the caller and central reusable workflow.

## Focused proof

```sh
cargo test --locked -p perl-rust-small-verifier
```

The library runs the unchanged canonical tests. The binary integration test uses
a disposable Git checkout and synthetic receipts to check CLI verification and
refusal; those fixtures are not proof that any product steps actually ran.
