//! Structured formatter property fuzz target (#10301, FPH-010).
//!
//! Raw fuzz input never becomes Perl source and is never executed: the first
//! eight bytes select a deterministic seed, the ninth byte selects a case
//! index and whether the case is a deliberately invalid negative control.
//! Both paths drive the exact same invariant checker used by the ordinary
//! package properties
//! (`crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`),
//! included here verbatim via `#[path]` so the fuzz and property tiers cannot
//! drift apart. #10301 remains open; this branch lands a bounded subset.
//! Predetermined replay-control vectors exercise the valid path, invalidation
//! path, and an index >= 16 through the same decoder. No runtime fuzzing
//! campaign has been executed, so crash-derived corpus evidence is not proven.
//! The sole panic invocation below is the intentional libFuzzer crash signal
//! and is exempted from the FPH-009 forbidden-construct scan for that reason.
#![no_main]
// The shared harness module is included verbatim; the fuzz entry point only
// exercises its generation-and-checker surface.
#![allow(dead_code)]

#[path = "../../crates/perl-lsp-perltidy/tests/support/formatter_property_harness/mod.rs"]
mod formatter_property_harness;

use formatter_property_harness::{case_from_fuzz_input, run_case};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The decode lives in the shared harness core so replay controls exercise
    // the exact same `(seed, selector)` mapping.
    let Some(case) = case_from_fuzz_input(data) else { return };

    if let Err(violation) = run_case(&case) {
        panic!(
            "formatter property violation (structured seed {}, family {}, disposition {}): {violation}",
            case.seed,
            case.family.name(),
            case.disposition
        );
    }
});
