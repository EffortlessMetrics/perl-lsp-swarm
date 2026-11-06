# Acceptance: #11458

- Exactly one PR establishes exactly the proposition above.
- First falsifiers fail closed: #11458 dimension-exceeded mutants.
- Positive proof observed: cargo test -p xtask --all-targets --locked (mutation suite over generated and curated surfaces).
- Broader affected-package proof observed before merge; failures are named, not skipped.
- Claim/evidence stage recorded honestly against the ceiling above.
- Successor unblocked: #11461 runs publication-context parity.
