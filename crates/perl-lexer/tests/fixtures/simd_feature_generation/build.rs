fn emit_generated_source() {
    let key = concat!("OUT", "_DIR");
    let _out_dir = std::env::var_os(key);
}
