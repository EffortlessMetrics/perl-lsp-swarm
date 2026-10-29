# Evidence and review

Production owner SHA256:
`3e903e4b53dc28073276f3cf56ca610f8bb9b5a096a849c750433437d316eddb`.

Final focused command used installed direct 1.95.0 Cargo on PATH, explicit cloud
CARGO_HOME/RUSTUP_HOME, RUSTUP_AUTO_INSTALL=0,
CARGO_ADMITTED_REAL_BUILD_TEST=1 and CLIPPY_CONTRACT_TOOLCHAIN_ROOT, then:

```text
python3 -m unittest scripts.tests.test_cargo_admitted_storage
  scripts.tests.test_cargo_admitted_budget
  scripts.tests.test_cargo_admitted_builtin_tree
  scripts.tests.test_cargo_admitted_clippy
  scripts.tests.test_cargo_admitted_clippy_tree
  scripts.tests.test_clippy_driver_contract -q
```

110 tests passed, none skipped, in 6.007 seconds. Includes 9 new builtin controls,
25 Clippy route controls, 12 existing native lifecycle controls, 10 installed
driver controls and the real offline tiny A/B/A build. The A/B/A test restores
actual production owner/launcher functions rather than its ordinary storage
policy mocks, asserting kernel proof and release at each operation. Its capacity
observation is deliberately mocked for an identity-only fixture, not real host
budget qualification. No consumer/full-workspace Rust build was run.

The original implementation was executed against the new native discriminator:
both 0 and 101 returned while the detached helper was alive and lease absent;
the short-deadline control returned 0 instead of 75. Three assertion failures,
zero instrument errors. Re-execution against immutable known-bad owner 8a375c220
after final fixture hardening reproduced the same three discriminating failures.
An independent legacy-route mutant also failed both helper controls. Finite test
helpers were reaped by their exclusive outer fixture owner; no host scanning.

Positive native controls assert the helper observes its lease still present
before exiting, then the inner owner records ECHILD and actual release. A bounded
short drain retains the lease; an outer test owner settles the remaining finite
fixture helper. Actual SIGTERM returns 130 after closure. Additional controls
cover product 0/101/17/-9, failed release, cancellation, missing capability with
no fallback and legacy non-Linux normal/abnormal/interruption behavior.

Fresh independent ownership/result-plane and proof/compatibility reviews ran the
affected tests. They found unconditional Linux status expectations in two old
storage fixtures that would regress legacy platform testing; these were repaired
with explicit platform-conditioned expectations, then simulated legacy controls
passed. Final judgments and proof hashes are recorded in evidence.json. No new
production ownership defect or duplicate lifecycle authority was found.

Artifacts from old attempts, the old consumer lease, shared caches/toolchain and
storage #17478 production branch remain untouched. Temporary finite unit fixtures
are test-owned; the surviving-helper tests settle their outer scope before fixture
disposal. No retroactive old-resource proof is claimed. Full canonical output,
typed observation readback, native Windows and hosted qualification remain outside
this packet. Quiet push only; no PR/Ready/merge or expensive hosted CI.
