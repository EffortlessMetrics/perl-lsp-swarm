#[cfg(feature="simd")]
fn select_simd_implementation() {}

fn selected_at_macro_site() -> bool {
    cfg!(feature="simd")
}
