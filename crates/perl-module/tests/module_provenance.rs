use perl_module::{
    ModuleProvenance, ModuleProvenanceClass, detect_module_provenance, module_provenance_root,
};
use std::fs;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn write_module(root: &Path, relative: &str) -> TestResult {
    let module = root.join(relative);
    fs::create_dir_all(module.parent().ok_or("module parent")?)?;
    fs::write(&module, "package Foo;\n")?;
    Ok(())
}

#[test]
fn detects_markers_at_distribution_root() -> TestResult {
    let temp = tempfile::tempdir()?;
    let module = temp.path().join("lib/Foo/Bar.pm");
    fs::create_dir_all(module.parent().ok_or("module parent")?)?;
    fs::write(&module, "package Foo::Bar;\n")?;
    fs::write(temp.path().join("META.json"), "{}")?;
    fs::write(temp.path().join("SIGNATURE"), "signature")?;
    fs::write(temp.path().join("CHECKSUMS"), "checksums")?;

    let provenance = detect_module_provenance(&module, temp.path());
    assert_eq!(
        provenance,
        ModuleProvenance { has_meta: true, has_signature: true, has_checksums: true }
    );
    assert_eq!(provenance.class(), ModuleProvenanceClass::ClaimsSignature);
    assert_eq!(module_provenance_root(&module, temp.path()), Some(temp.path().to_path_buf()));
    Ok(())
}

#[test]
fn unmarked_or_missing_modules_are_unknown() -> TestResult {
    let temp = tempfile::tempdir()?;
    write_module(temp.path(), "lib/Foo.pm")?;

    assert_eq!(
        detect_module_provenance(temp.path().join("lib/Foo.pm").as_path(), temp.path()),
        ModuleProvenance::default()
    );
    assert_eq!(module_provenance_root(temp.path().join("lib/Foo.pm").as_path(), temp.path()), None);
    Ok(())
}

#[test]
fn recognizes_each_marker_independently() -> TestResult {
    for (marker, expected) in [
        (
            "META.yml",
            ModuleProvenance { has_meta: true, has_signature: false, has_checksums: false },
        ),
        (
            "SIGNATURE",
            ModuleProvenance { has_meta: false, has_signature: true, has_checksums: false },
        ),
        (
            "CHECKSUMS",
            ModuleProvenance { has_meta: false, has_signature: false, has_checksums: true },
        ),
    ] {
        let temp = tempfile::tempdir()?;
        let module = temp.path().join("lib/Foo.pm");
        fs::create_dir_all(module.parent().ok_or("module parent")?)?;
        fs::write(&module, "package Foo;\n")?;
        fs::write(temp.path().join(marker), "marker")?;

        assert_eq!(detect_module_provenance(&module, temp.path()), expected);
        assert_eq!(module_provenance_root(&module, temp.path()), Some(temp.path().to_path_buf()));
    }
    Ok(())
}

#[test]
fn does_not_attribute_ancestors_to_missing_or_directory_paths() -> TestResult {
    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("lib/Foo"))?;
    fs::write(temp.path().join("META.json"), "{}")?;

    let missing = temp.path().join("lib/Foo/Missing.pm");
    let directory = temp.path().join("lib/Foo");
    assert_eq!(detect_module_provenance(&missing, temp.path()), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&missing, temp.path()), None);
    assert_eq!(detect_module_provenance(&directory, temp.path()), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&directory, temp.path()), None);
    Ok(())
}

/// A `CHECKSUMS`-only distribution is recognized metadata: it classifies as
/// `Packaged`, never as `Unknown`. Recognized checksum metadata must not be
/// indistinguishable from absence.
#[test]
fn checksums_only_modules_are_packaged() -> TestResult {
    let temp = tempfile::tempdir()?;
    write_module(temp.path(), "lib/Foo.pm")?;
    fs::write(temp.path().join("CHECKSUMS"), "checksums")?;

    let module = temp.path().join("lib/Foo.pm");
    let provenance = detect_module_provenance(&module, temp.path());
    assert_eq!(provenance, ModuleProvenance { has_checksums: true, ..ModuleProvenance::default() });
    assert_eq!(
        provenance.class(),
        ModuleProvenanceClass::Packaged,
        "CHECKSUMS-only distributions are Packaged, not Unknown"
    );
    Ok(())
}

/// The upward walk is anchored at the caller-supplied authority root: an
/// unmarked module inside the root must not inherit a marker sitting
/// immediately above the admitted boundary, even though an unbounded walk
/// would find it.
#[test]
fn markers_above_the_authority_root_are_not_inherited() -> TestResult {
    let temp = tempfile::tempdir()?;
    let admitted = temp.path().join("vendor/lib");
    let module = admitted.join("Foo.pm");
    fs::create_dir_all(&admitted)?;
    fs::write(&module, "package Foo;\n")?;
    // Unrelated marker one directory above the admitted root.
    fs::write(temp.path().join("META.json"), "{}")?;

    assert_eq!(detect_module_provenance(&module, &admitted), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&module, &admitted), None);
    // Control: the same marker IS found when the boundary admits it.
    assert_eq!(
        detect_module_provenance(&module, temp.path()).class(),
        ModuleProvenanceClass::Packaged
    );
    Ok(())
}

/// The authority root itself is inside the boundary: a marker at the root of
/// an admitted tree is attributed, while the walk stops there.
#[test]
fn marker_at_the_authority_root_is_found_and_ends_the_walk() -> TestResult {
    let temp = tempfile::tempdir()?;
    write_module(temp.path(), "lib/Foo.pm")?;
    fs::write(temp.path().join("CHECKSUMS"), "checksums at the boundary")?;

    let module = temp.path().join("lib/Foo.pm");
    let provenance = detect_module_provenance(&module, temp.path());
    assert!(provenance.has_checksums);
    assert_eq!(module_provenance_root(&module, temp.path()), Some(temp.path().to_path_buf()));
    Ok(())
}

/// A module outside the authority root gets no attribution at all: the
/// boundary is a precondition, not a filter on results.
#[test]
fn module_outside_the_authority_root_has_no_provenance() -> TestResult {
    let inside = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let module = outside.path().join("lib/Foo.pm");
    fs::create_dir_all(module.parent().ok_or("module parent")?)?;
    fs::write(&module, "package Foo;\n")?;
    fs::write(outside.path().join("META.json"), "{}")?;

    assert_eq!(detect_module_provenance(&module, inside.path()), ModuleProvenance::default());
    assert_eq!(module_provenance_root(&module, inside.path()), None);
    Ok(())
}
