fn reads_legacy_feature() {
    let prefix = "CARGO_FEATURE_";
    let suffix = "SIMD";
    let key = format!("{prefix}{suffix}");
    let _ = std::env::var_os(key);
}
