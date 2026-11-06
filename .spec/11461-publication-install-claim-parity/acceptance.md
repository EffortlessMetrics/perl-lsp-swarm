# Acceptance: #11461

- Exactly one PR establishes exactly the proposition above.
- First falsifiers fail closed: #11461 stage-substitution mutants.
- Positive proof observed: cargo test -p xtask --all-targets --locked (projected-tree fixture matrix, differential controls).
- Broader affected-package proof observed before merge; failures are named, not skipped.
- Claim/evidence stage recorded honestly against the ceiling above.
- Successor unblocked: #11463 cut over; #4350 consumes the result without parsing prose.
