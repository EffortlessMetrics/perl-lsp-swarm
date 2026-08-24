use perl_module::{
    ModuleProvenance, ModuleProvenanceClass, detect_module_provenance, module_provenance_root,
};
use std::fs;

#[test]
fn detects_markers_at_distribution_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let module = temp.path().join("lib/Foo/Bar.pm");
    fs::create_dir_all(module.parent().expect("module parent")).expect("module directory");
    fs::write(&module, "package Foo::Bar;\n").expect("module");
    fs::write(temp.path().join("META.json"), "{}").expect("meta");
    fs::write(temp.path().join("SIGNATURE"), "signature").expect("signature");
    fs::write(temp.path().join("CHECKSUMS"), "checksums").expect("checksums");

    let provenance = detect_module_provenance(&module);
    assert_eq!(
        provenance,
        ModuleProvenance { has_meta: true, has_signature: true, has_checksums: true }
    );
    assert_eq!(provenance.class(), ModuleProvenanceClass::ClaimsSignature);
    assert_eq!(module_provenance_root(&module), Some(temp.path().to_path_buf()));
}

#[test]
fn unmarked_or_missing_modules_are_unknown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let module = temp.path().join("lib/Foo.pm");
    fs::create_dir_all(module.parent().expect("module parent")).expect("module directory");
    fs::write(&module, "package Foo;\n").expect("module");

    assert_eq!(detect_module_provenance(&module), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&module), None);
}

#[test]
fn recognizes_each_marker_independently() {
    for (marker, expected) in [
        ("META.yml", ModuleProvenance { has_meta: true, has_signature: false, has_checksums: false }),
        ("SIGNATURE", ModuleProvenance { has_meta: false, has_signature: true, has_checksums: false }),
        ("CHECKSUMS", ModuleProvenance { has_meta: false, has_signature: false, has_checksums: true }),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let module = temp.path().join("lib/Foo.pm");
        fs::create_dir_all(module.parent().expect("module parent")).expect("module directory");
        fs::write(&module, "package Foo;\n").expect("module");
        fs::write(temp.path().join(marker), "marker").expect("marker");

        assert_eq!(detect_module_provenance(&module), expected);
        assert_eq!(module_provenance_root(&module), Some(temp.path().to_path_buf()));
    }
}

#[test]
fn does_not_attribute_ancestors_to_missing_or_directory_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("lib/Foo")).expect("module directory");
    fs::write(temp.path().join("META.json"), "{}").expect("meta");

    let missing = temp.path().join("lib/Foo/Missing.pm");
    let directory = temp.path().join("lib/Foo");
    assert_eq!(detect_module_provenance(&missing), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&missing), None);
    assert_eq!(detect_module_provenance(&directory), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&directory), None);
}
