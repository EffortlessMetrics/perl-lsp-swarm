//! Repository-level integrity and population gates for `test_corpus/gold`.
//!
//! Verify with:
//!
//! ```bash
//! cargo test -p perl-corpus --test gold_repository_contract
//! ```

use perl_corpus::gold::{GoldAssertion, GoldExpected};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const MIN_FIXTURE_DIRECTORIES: usize = 34;

const SIDECAR_FLOORS: [(&str, usize); 7] = [
    ("expected.json", 28),
    ("expected_hover.json", 8),
    ("expected_goto.json", 3),
    ("expected_completion.json", 4),
    ("expected_symbols.json", 2),
    ("expected_rename.json", 2),
    ("expected_module.json", 5),
];

fn contract_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

fn gold_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| contract_error("perl-corpus must live under <workspace>/crates"))?;
    Ok(workspace_root.join("test_corpus").join("gold"))
}

fn fixture_name(directory: &Path) -> Result<String, Box<dyn Error>> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| contract_error(format!("invalid fixture directory name: {}", directory.display())))?;
    Ok(name.to_owned())
}

fn fixture_directories(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut directories = Vec::new();

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_symlink() {
            return Err(contract_error(format!(
                "gold corpus root contains a symbolic link: {}",
                path.display()
            ))
            .into());
        }
        if file_type.is_dir() {
            directories.push(path);
            continue;
        }
        if name != "README.md" {
            return Err(contract_error(format!(
                "unexpected top-level gold corpus asset: {}",
                path.display()
            ))
            .into());
        }
    }

    directories.sort();
    Ok(directories)
}

fn require_regular_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        contract_error(format!("required corpus file {}: {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() {
        return Err(contract_error(format!(
            "corpus asset must be a regular file: {}",
            path.display()
        ))
        .into());
    }
    Ok(())
}

fn regular_file_if_present(path: &Path) -> Result<bool, Box<dyn Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(contract_error(format!(
            "corpus sidecar must be a regular file: {}",
            path.display()
        ))
        .into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(contract_error(format!(
            "reading corpus sidecar metadata {}: {error}",
            path.display()
        ))
        .into()),
    }
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text)
        .map_err(|error| contract_error(format!("parsing {}: {error}", path.display())).into())
}

fn validate_diagnostics_sidecar(path: &Path, source_len: usize) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let expected: GoldExpected = serde_json::from_str(&text)
        .map_err(|error| contract_error(format!("parsing {}: {error}", path.display())))?;

    assert!(
        !expected.diagnostics.is_empty(),
        "{} must contain at least one diagnostics assertion",
        path.display()
    );

    for assertion in &expected.diagnostics {
        if let GoldAssertion::DiagnosticPresent {
            byte_offset: Some(byte_offset),
            ..
        } = assertion
        {
            assert!(
                *byte_offset <= source_len,
                "{} declares byte_offset {} beyond fixture length {}",
                path.display(),
                byte_offset,
                source_len
            );
        }
    }

    Ok(())
}

fn validate_named_sidecar(path: &Path, expected_fixture: &str) -> Result<(), Box<dyn Error>> {
    let document = read_json(path)?;
    let object = document.as_object().ok_or_else(|| {
        contract_error(format!("{} must contain a JSON object", path.display()))
    })?;

    let version = object.get("version").and_then(Value::as_u64).ok_or_else(|| {
        contract_error(format!("{} must declare an integer version", path.display()))
    })?;
    assert_eq!(version, 1, "{} uses an unsupported sidecar version", path.display());

    let declared_fixture = object.get("fixture").and_then(Value::as_str).ok_or_else(|| {
        contract_error(format!("{} must declare its fixture identity", path.display()))
    })?;
    assert_eq!(
        declared_fixture,
        expected_fixture,
        "{} fixture identity must match its directory",
        path.display()
    );

    let assertions = object.get("assertions").and_then(Value::as_array).ok_or_else(|| {
        contract_error(format!("{} must declare an assertions array", path.display()))
    })?;
    assert!(
        !assertions.is_empty(),
        "{} must contain at least one assertion",
        path.display()
    );

    Ok(())
}

fn reject_unknown_sidecars(directory: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("expected")
            && name.ends_with(".json")
            && !SIDECAR_FLOORS.iter().any(|(known, _)| name.as_str() == *known)
        {
            return Err(contract_error(format!(
                "unregistered gold sidecar {} in {}",
                name,
                directory.display()
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn gold_repository_assets_are_complete_and_population_does_not_regress()
-> Result<(), Box<dyn Error>> {
    let root = gold_root()?;
    let root_metadata = fs::symlink_metadata(&root)?;
    assert!(
        root_metadata.file_type().is_dir(),
        "gold corpus root must be a real directory: {}",
        root.display()
    );

    let directories = fixture_directories(&root)?;
    assert!(
        directories.len() >= MIN_FIXTURE_DIRECTORIES,
        "gold corpus shrank to {} fixture directories; floor is {}",
        directories.len(),
        MIN_FIXTURE_DIRECTORIES
    );

    let mut sidecar_counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for directory in &directories {
        reject_unknown_sidecars(directory)?;

        let name = fixture_name(directory)?;
        let fixture_path = directory.join("fixture.pl");
        require_regular_file(&fixture_path)?;
        let source = fs::read_to_string(&fixture_path)?;

        let mut fixture_sidecars = 0usize;
        for (sidecar, _) in SIDECAR_FLOORS {
            let path = directory.join(sidecar);
            if !regular_file_if_present(&path)? {
                continue;
            }

            fixture_sidecars += 1;
            *sidecar_counts.entry(sidecar).or_insert(0) += 1;

            if sidecar == "expected.json" {
                validate_diagnostics_sidecar(&path, source.len())?;
            } else {
                validate_named_sidecar(&path, &name)?;
            }
        }

        assert!(
            fixture_sidecars > 0,
            "{} has fixture.pl but no recognized assertion sidecar",
            directory.display()
        );
    }

    for (sidecar, floor) in SIDECAR_FLOORS {
        let count = sidecar_counts.get(sidecar).copied().unwrap_or_default();
        assert!(
            count >= floor,
            "gold corpus {sidecar} population regressed to {count}; floor is {floor}"
        );
    }

    Ok(())
}
