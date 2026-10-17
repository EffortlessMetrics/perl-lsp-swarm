# Acceptance: #11449

- Exactly one PR establishes exactly the proposition above.
- First falsifiers fail closed: #11449 marker/context/inventory mutants.
- Positive proof observed: cargo test -p xtask --all-targets --locked (renderer/marker fixtures, second-run clean).
- Broader affected-package proof observed before merge; failures are named, not skipped.
- Claim/evidence stage recorded honestly against the ceiling above.
- Successor unblocked: G01 #11451, G02 #11453, G03 #11454 migrate rows atomically with their surfaces.
