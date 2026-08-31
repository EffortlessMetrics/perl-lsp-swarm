fn reads_legacy_feature() {
    let _ = std::env::var_os("CARGO_FEATURE_SIMD");
}
