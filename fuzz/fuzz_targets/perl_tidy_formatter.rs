//! Structured formatter property fuzz target (#10301, FPH-010).
//!
//! Raw fuzz input never becomes Perl source and is never executed: the first
//! eight bytes select a deterministic seed, the ninth byte selects a case
//! index and whether the case is a deliberately invalid negative control.
//! Both paths drive the exact same invariant checker used by the ordinary
//! package properties
//! (`crates/perl-lsp-perltidy/tests/support/formatter_property_harness/`),
//! included here verbatim via `#[path]` so the fuzz and property tiers cannot
//! drift apart. Any violated invariant is a crash; minimized crashes shrink
//! to a `(seed, index)` pair that is committed as a focused regression entry
//! in the crate's `.proptest-regressions` convention.
#![no_main]
// The shared harness module is included verbatim; the fuzz entry point only
// exercises its generation-and-checker surface.
#![allow(dead_code)]

#[path = "../../crates/perl-lsp-perltidy/tests/support/formatter_property_harness/mod.rs"]
mod formatter_property_harness;

use formatter_property_harness::{generate_case, generate_invalidation_case, run_case};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 9 {
        return;
    }

    let mut seed_bytes = [0_u8; 8];
    seed_bytes.copy_from_slice(&data[..8]);
    let seed = u64::from_le_bytes(seed_bytes);
    let selector = data[8];
    let index = usize::from(selector & 0x3f) % 64;

    let case = if selector & 0x80 != 0 {
        generate_invalidation_case(seed, index)
    } else {
        generate_case(seed, index)
    };

    if let Err(violation) = run_case(&case) {
        panic!(
            "formatter property violation (structured seed {seed}, index {index}): {violation}"
        );
    }
});
