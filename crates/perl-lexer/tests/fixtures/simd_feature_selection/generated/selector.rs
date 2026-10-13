#[cfg(feature /* separator */ = /* separator */ "simd")]
fn select_simd_implementation() {}

fn selected_at_macro_site() -> bool {
    cfg!(feature /* separator */ = /* separator */ "simd")
}
