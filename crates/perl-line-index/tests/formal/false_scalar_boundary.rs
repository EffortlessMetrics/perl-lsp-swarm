//! Deliberately false theorem; select separately, never as a positive obligation.
//! The expected failure is FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY, not tool error.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("The initial position proof is scoped to 64-bit usize.");

#[path = "../../src/lib.rs"]
mod production;
use production::LineIndex;

mod contract;

#[kani::proof]
#[kani::unwind(32)]
fn false_byte_scalar_boundary() {
    let case: u16 = kani::any();
    kani::assume(case < contract::CASE_COUNT);
    let storage = contract::case_text(case);
    let decoded = storage.as_str();
    assert!(decoded.is_ok(), "FV_POSITION_GENERATOR_UTF8");
    if let Ok(text) = decoded {
        let index = LineIndex::new(text);
        let offset: usize = kani::any();
        let (line, column) = index.byte_to_position(offset);
        if let Some(byte) = index.position_to_byte(line, column) {
            assert!(text.is_char_boundary(byte), "FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY");
        }
    }
}
