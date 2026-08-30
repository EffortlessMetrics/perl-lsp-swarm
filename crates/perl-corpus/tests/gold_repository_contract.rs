//! Repository-level integrity and population gates for `test_corpus/gold`.
//!
//! Verify with:
//!
//! ```bash
//! cargo test -p perl-corpus --test gold_repository_contract
//! ```

use perl_corpus::gold::{
    CompletionGoldExpected, DocumentSymbolGoldExpected, GoldAssertion, GoldExpected,
    GotoGoldExpected, HoverGoldExpected, RenameGoldExpected,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
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

const FIXTURE_FILES: [&str; 8] = [
    "fixture.pl",
    "expected.json",
    "expected_hover.json",
    "expected_goto.json",
    "expected_completion.json",
    "expected_symbols.json",
    "expected_rename.json",
    "expected_module.json",
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
    let name = directory.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        contract_error(format!("invalid fixture directory name: {}", directory.display()))
    })?;
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
    let document = serde_json::from_str(&text)
        .map_err(|error| contract_error(format!("parsing {}: {error}", path.display())))?;
    Ok(document)
}

fn validate_diagnostics_sidecar(path: &Path, source_len: usize) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let expected: GoldExpected = serde_json::from_str(&text)
        .map_err(|error| contract_error(format!("parsing {}: {error}", path.display())))?;

    if expected.diagnostics.is_empty() {
        return Err(contract_error(format!(
            "{} must contain at least one diagnostics assertion",
            path.display()
        ))
        .into());
    }

    for assertion in &expected.diagnostics {
        if let GoldAssertion::DiagnosticPresent { byte_offset: Some(byte_offset), .. } = assertion {
            if *byte_offset > source_len {
                return Err(contract_error(format!(
                    "{} declares byte_offset {} beyond fixture length {}",
                    path.display(),
                    byte_offset,
                    source_len
                ))
                .into());
            }
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleGoldExpected {
    version: u32,
    fixture: String,
    resolution_mode: String,
    assertions: Vec<ModuleAssertion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
enum ModuleAssertion {
    Resolves {
        module: String,
        expected_suffix: String,
        use_line: u32,
        use_col: u32,
        consumers: Vec<String>,
        #[serde(default)]
        rationale: String,
    },
    NotResolved {
        module: String,
        use_line: u32,
        use_col: u32,
        consumers: Vec<String>,
        #[serde(default)]
        rationale: String,
    },
}

fn validate_typed_named_sidecar<T: DeserializeOwned>(
    path: &Path,
    document: &Value,
) -> Result<(), Box<dyn Error>> {
    serde_json::from_value::<T>(document.clone()).map_err(|error| {
        contract_error(format!("typed assertions in {} are invalid: {error}", path.display()))
    })?;
    Ok(())
}

fn validate_module_sidecar(
    path: &Path,
    document: &Value,
    expected_fixture: &str,
) -> Result<(), Box<dyn Error>> {
    let expected: ModuleGoldExpected =
        serde_json::from_value(document.clone()).map_err(|error| {
            contract_error(format!("typed assertions in {} are invalid: {error}", path.display()))
        })?;

    if expected.version != 1 {
        return Err(contract_error(format!(
            "{} uses an unsupported sidecar version",
            path.display()
        ))
        .into());
    }
    if expected.fixture != expected_fixture {
        return Err(contract_error(format!(
            "{} fixture identity must match its directory",
            path.display()
        ))
        .into());
    }
    if expected.resolution_mode.trim().is_empty() {
        return Err(contract_error(format!(
            "{} must declare a non-empty resolution mode",
            path.display()
        ))
        .into());
    }
    if expected.assertions.is_empty() {
        return Err(contract_error(format!(
            "{} must contain at least one assertion",
            path.display()
        ))
        .into());
    }

    for assertion in expected.assertions {
        let (module, consumers, rationale) = match assertion {
            ModuleAssertion::Resolves { module, consumers, rationale, .. }
            | ModuleAssertion::NotResolved { module, consumers, rationale, .. } => {
                (module, consumers, rationale)
            }
        };
        if module.trim().is_empty() {
            return Err(contract_error(format!(
                "{} contains an assertion with an empty module",
                path.display()
            ))
            .into());
        }
        if consumers.is_empty() || consumers.iter().any(|consumer| consumer.trim().is_empty()) {
            return Err(contract_error(format!(
                "{} contains an assertion with invalid consumers",
                path.display()
            ))
            .into());
        }
        let _ = rationale;
    }

    Ok(())
}

fn validate_named_sidecar(path: &Path, expected_fixture: &str) -> Result<(), Box<dyn Error>> {
    let document = read_json(path)?;
    let object = document
        .as_object()
        .ok_or_else(|| contract_error(format!("{} must contain a JSON object", path.display())))?;

    let version = object.get("version").and_then(Value::as_u64).ok_or_else(|| {
        contract_error(format!("{} must declare an integer version", path.display()))
    })?;
    if version != 1 {
        return Err(contract_error(format!(
            "{} uses an unsupported sidecar version",
            path.display()
        ))
        .into());
    }

    let declared_fixture = object.get("fixture").and_then(Value::as_str).ok_or_else(|| {
        contract_error(format!("{} must declare its fixture identity", path.display()))
    })?;
    if declared_fixture != expected_fixture {
        return Err(contract_error(format!(
            "{} fixture identity must match its directory",
            path.display()
        ))
        .into());
    }

    let assertions = object.get("assertions").and_then(Value::as_array).ok_or_else(|| {
        contract_error(format!("{} must declare an assertions array", path.display()))
    })?;
    if assertions.is_empty() {
        return Err(contract_error(format!(
            "{} must contain at least one assertion",
            path.display()
        ))
        .into());
    }

    match path.file_name().and_then(|name| name.to_str()) {
        Some("expected_hover.json") => {
            validate_typed_named_sidecar::<HoverGoldExpected>(path, &document)?;
        }
        Some("expected_goto.json") => {
            validate_typed_named_sidecar::<GotoGoldExpected>(path, &document)?;
        }
        Some("expected_completion.json") => {
            validate_typed_named_sidecar::<CompletionGoldExpected>(path, &document)?;
        }
        Some("expected_symbols.json") => {
            validate_typed_named_sidecar::<DocumentSymbolGoldExpected>(path, &document)?;
        }
        Some("expected_rename.json") => {
            validate_typed_named_sidecar::<RenameGoldExpected>(path, &document)?;
        }
        Some("expected_module.json") => {
            validate_module_sidecar(path, &document, expected_fixture)?;
        }
        Some("expected.json") | None => {}
        Some(name) => {
            return Err(
                contract_error(format!("{} is not a recognized named sidecar", name)).into()
            );
        }
    }

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

fn validate_module_payload(path: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let member = entry.path();
        let metadata = fs::symlink_metadata(&member)?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if metadata.file_type().is_symlink() {
            return Err(contract_error(format!(
                "module fixture payload contains a symbolic link: {}",
                member.display()
            ))
            .into());
        }
        if metadata.file_type().is_dir() {
            validate_module_payload(&member)?;
            continue;
        }
        if !metadata.file_type().is_file()
            || Path::new(&name).extension().and_then(|ext| ext.to_str()) != Some("pm")
        {
            return Err(contract_error(format!(
                "module fixture payload must contain only regular .pm files: {}",
                member.display()
            ))
            .into());
        }
    }
    Ok(())
}

fn validate_fixture_members(directory: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;

        if metadata.file_type().is_symlink() {
            return Err(contract_error(format!(
                "fixture directory contains a symbolic link: {}",
                path.display()
            ))
            .into());
        }
        if name == "lib" {
            if !metadata.file_type().is_dir() {
                return Err(contract_error(format!(
                    "fixture lib payload must be a directory: {}",
                    path.display()
                ))
                .into());
            }
            validate_module_payload(&path)?;
            continue;
        }
        if !FIXTURE_FILES.contains(&name.as_str()) {
            return Err(
                contract_error(format!("unexpected fixture asset: {}", path.display())).into()
            );
        }
        if !metadata.file_type().is_file() {
            return Err(contract_error(format!(
                "fixture member must be a regular file: {}",
                path.display()
            ))
            .into());
        }
    }
    Ok(())
}

fn validate_fixture_directory(
    directory: &Path,
    sidecar_counts: &mut BTreeMap<&'static str, usize>,
) -> Result<(), Box<dyn Error>> {
    validate_fixture_members(directory)?;
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

    if fixture_sidecars == 0 {
        return Err(contract_error(format!(
            "{} has fixture.pl but no recognized assertion sidecar",
            directory.display()
        ))
        .into());
    }

    Ok(())
}

#[test]
fn gold_repository_contract_holds() -> Result<(), Box<dyn Error>> {
    let root = gold_root()?;
    let root_metadata = fs::symlink_metadata(&root)?;
    if !root_metadata.file_type().is_dir() {
        return Err(contract_error(format!(
            "gold corpus root must be a real directory: {}",
            root.display()
        ))
        .into());
    }

    let directories = fixture_directories(&root)?;
    if directories.len() < MIN_FIXTURE_DIRECTORIES {
        return Err(contract_error(format!(
            "gold corpus shrank to {} fixture directories; floor is {}",
            directories.len(),
            MIN_FIXTURE_DIRECTORIES
        ))
        .into());
    }

    let mut sidecar_counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for directory in &directories {
        validate_fixture_directory(directory, &mut sidecar_counts)?;
    }

    for (sidecar, floor) in SIDECAR_FLOORS {
        let count = sidecar_counts.get(sidecar).copied().unwrap_or_default();
        if count < floor {
            return Err(contract_error(format!(
                "gold corpus {sidecar} population regressed to {count}; floor is {floor}"
            ))
            .into());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_file_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)?;
            return Ok(true);
        }

        #[cfg(windows)]
        {
            if perl_tdd_support::symlink_test_decision().skip_visibly() {
                return Ok(false);
            }
            return Ok(perl_tdd_support::try_create_file_symlink(target, link)?.is_some());
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            Err(contract_error("symlink contract controls are unsupported on this platform").into())
        }
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> Result<bool, Box<dyn Error>> {
        if perl_tdd_support::symlink_test_decision().skip_visibly() {
            return Ok(false);
        }
        Ok(perl_tdd_support::try_create_dir_symlink(target, link)?.is_some())
    }

    fn write_fixture_file(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
        fs::write(path, contents)?;
        Ok(())
    }

    #[test]
    fn rejects_unknown_expected_sidecars() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected_future.json");
        write_fixture_file(&sidecar, "{}")?;

        let error = match reject_unknown_sidecars(directory.path()) {
            Ok(()) => return Err(contract_error("unknown expected sidecar was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("unregistered gold sidecar") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_fixture_without_a_recognized_sidecar() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("missing_sidecar");
        fs::create_dir(&fixture)?;
        write_fixture_file(&fixture.join("fixture.pl"), "use strict;\n")?;

        let directories = fixture_directories(root.path())?;
        let directory = directories.first().ok_or("fixture directory was not discovered")?;
        let mut sidecar_counts = BTreeMap::new();
        let error = match validate_fixture_directory(directory, &mut sidecar_counts) {
            Ok(()) => return Err(contract_error("fixture without a sidecar was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("no recognized assertion sidecar") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_named_sidecar_metadata() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        for (contents, expected_message) in [
            (
                r#"{"version":2,"fixture":"fixture","assertions":[{}]}"#,
                "unsupported sidecar version",
            ),
            (r#"{"version":1,"fixture":"other","assertions":[{}]}"#, "fixture identity must match"),
            (r#"{"version":1,"fixture":"fixture","assertions":[]}"#, "at least one assertion"),
        ] {
            let sidecar = directory.path().join("expected_hover.json");
            write_fixture_file(&sidecar, contents)?;
            let error = match validate_named_sidecar(&sidecar, "fixture") {
                Ok(()) => return Err(contract_error("invalid named sidecar was accepted").into()),
                Err(error) => error,
            };
            if !error.to_string().contains(expected_message) {
                return Err(contract_error(format!("unexpected validation error: {error}")).into());
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_out_of_bounds_diagnostic_offsets() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected.json");
        write_fixture_file(
            &sidecar,
            r#"{"diagnostics":[{"assertion":"diagnostic_present","code":"PL001","byte_offset":2}]}"#,
        )?;

        let error = match validate_diagnostics_sidecar(&sidecar, 1) {
            Ok(()) => return Err(contract_error("out-of-bounds offset was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("beyond fixture length") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_malformed_typed_named_assertion_members() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected_hover.json");
        write_fixture_file(&sidecar, r#"{"version":1,"fixture":"fixture","assertions":[{}]}"#)?;

        let error = match validate_named_sidecar(&sidecar, "fixture") {
            Ok(()) => return Err(contract_error("malformed typed assertion was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("typed assertions") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn named_sidecars_accept_declared_fields_and_reject_unknown_fields()
    -> Result<(), Box<dyn Error>> {
        let cases = [
            (
                "expected_hover.json",
                r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"hover_contains","line":0,"character":0,"needle":"name","rationale":"known fields"}]}"#,
            ),
            (
                "expected_goto.json",
                r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"goto_line","line":0,"character":0,"expected_line":1,"rationale":"known fields"}]}"#,
            ),
            (
                "expected_completion.json",
                r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"completion_present","line":0,"character":0,"expected_label":"name","rationale":"known fields"}]}"#,
            ),
            (
                "expected_symbols.json",
                r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"symbol_present","name":"name","rationale":"known fields"}]}"#,
            ),
            (
                "expected_module.json",
                r#"{"version":1,"fixture":"fixture","resolution_mode":"test","assertions":[{"kind":"resolves","module":"Test","expected_suffix":"Test.pm","use_line":0,"use_col":4,"consumers":["goto_definition"],"rationale":"known fields"}]}"#,
            ),
        ];

        for (name, valid) in cases {
            let directory = tempdir()?;
            let sidecar = directory.path().join(name);
            write_fixture_file(&sidecar, valid)?;
            validate_named_sidecar(&sidecar, "fixture")?;

            let mut envelope: Value = serde_json::from_str(valid)?;
            let envelope_object =
                envelope.as_object_mut().ok_or("valid named sidecar must be an object")?;
            envelope_object.insert("unexpected_envelope".to_string(), Value::Bool(true));
            write_fixture_file(&sidecar, &serde_json::to_string(&envelope)?)?;
            let error = match validate_named_sidecar(&sidecar, "fixture") {
                Ok(()) => {
                    return Err(format!("unknown envelope field was accepted for {name}").into());
                }
                Err(error) => error,
            };
            if !error.to_string().contains("unexpected_envelope") {
                return Err(format!("unexpected envelope error for {name}: {error}").into());
            }

            let mut assertion: Value = serde_json::from_str(valid)?;
            let assertion_object = assertion
                .get_mut("assertions")
                .and_then(Value::as_array_mut)
                .and_then(|assertions| assertions.first_mut())
                .and_then(Value::as_object_mut)
                .ok_or("valid named sidecar must contain an assertion object")?;
            assertion_object.insert("unexpected_assertion".to_string(), Value::Bool(true));
            write_fixture_file(&sidecar, &serde_json::to_string(&assertion)?)?;
            let error = match validate_named_sidecar(&sidecar, "fixture") {
                Ok(()) => {
                    return Err(format!("unknown assertion field was accepted for {name}").into());
                }
                Err(error) => error,
            };
            if !error.to_string().contains("unexpected_assertion") {
                return Err(format!("unexpected assertion error for {name}: {error}").into());
            }
        }

        Ok(())
    }

    #[test]
    fn rejects_unknown_rename_assertion_fields_without_weakening_edit_modes()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected_rename.json");
        let omitted = r#"{
            "version": 1,
            "fixture": "fixture",
            "assertions": [{
                "kind": "rename_succeeds",
                "line": 4,
                "character": 4,
                "new_name": "sum_values"
            }]
        }"#;
        write_fixture_file(&sidecar, omitted)?;
        validate_named_sidecar(&sidecar, "fixture")?;
        let parsed: RenameGoldExpected = serde_json::from_str(omitted)?;
        if parsed
            .assertions
            .first()
            .and_then(|assertion| assertion.expected_edits.as_ref())
            .is_some()
        {
            return Err("omitted expected_edits must remain count-only mode".into());
        }

        let explicit_empty = omitted.replace(
            "\"new_name\": \"sum_values\"",
            "\"new_name\": \"sum_values\",\n                \"expected_edits\": []",
        );
        write_fixture_file(&sidecar, &explicit_empty)?;
        validate_named_sidecar(&sidecar, "fixture")?;
        let parsed: RenameGoldExpected = serde_json::from_str(&explicit_empty)?;
        if !parsed
            .assertions
            .first()
            .and_then(|assertion| assertion.expected_edits.as_ref())
            .is_some_and(Vec::is_empty)
        {
            return Err("explicit empty expected_edits must remain exact mode".into());
        }

        let typo = omitted.replace(
            "\"new_name\": \"sum_values\"",
            "\"new_name\": \"sum_values\",\n                \"expected_editz\": []",
        );
        write_fixture_file(&sidecar, &typo)?;
        let error = match validate_named_sidecar(&sidecar, "fixture") {
            Ok(()) => return Err("unknown rename assertion field was accepted".into()),
            Err(error) => error,
        };
        if !error.to_string().contains("expected_editz") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }

        let nested_typo = omitted.replace(
            "\"new_name\": \"sum_values\"",
            "\"new_name\": \"sum_values\",\n                \"expected_edits\": [{\n                    \"line\": 4,\n                    \"character\": 4,\n                    \"end_line\": 4,\n                    \"end_character\": 19,\n                    \"new_text\": \"sum_values\",\n                    \"new_te xt\": \"sum_values\"\n                }]",
        );
        write_fixture_file(&sidecar, &nested_typo)?;
        let error = match validate_named_sidecar(&sidecar, "fixture") {
            Ok(()) => return Err("unknown nested expected edit field was accepted".into()),
            Err(error) => error,
        };
        if !error.to_string().contains("new_te xt") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }

        for kind in ["rename_succeeds", "rename_null"] {
            let mismatched = format!(
                r#"{{"version":1,"fixture":"fixture","assertions":[{{"kind":"{kind}","line":4,"character":4,"new_name":"sum_values","min":1}}]}}"#
            );
            write_fixture_file(&sidecar, &mismatched)?;
            if validate_named_sidecar(&sidecar, "fixture").is_ok() {
                return Err(format!("min was accepted for {kind}").into());
            }
        }

        let rename_null_with_edits = r#"{
            "version": 1,
            "fixture": "fixture",
            "assertions": [{
                "kind": "rename_null",
                "line": 4,
                "character": 4,
                "new_name": "sum_values",
                "expected_edits": []
            }]
        }"#;
        write_fixture_file(&sidecar, rename_null_with_edits)?;
        if validate_named_sidecar(&sidecar, "fixture").is_ok() {
            return Err("expected_edits was accepted for rename_null".into());
        }

        let count = r#"{
            "version": 1,
            "fixture": "fixture",
            "assertions": [{
                "kind": "rename_edit_count_at_least",
                "min": 1,
                "line": 4,
                "character": 4,
                "new_name": "sum_values"
            }]
        }"#;
        write_fixture_file(&sidecar, count)?;
        validate_named_sidecar(&sidecar, "fixture")?;

        Ok(())
    }

    #[test]
    fn rejects_unclaimed_fixture_members() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("fixture");
        fs::create_dir(&fixture)?;
        write_fixture_file(&fixture.join("fixture.pl"), "use strict;\n")?;
        write_fixture_file(
            &fixture.join("expected.json"),
            r#"{"diagnostics":[{"assertion":"no_diagnostics"}]}"#,
        )?;
        fs::create_dir(fixture.join("unclaimed"))?;

        let error = match validate_fixture_members(&fixture) {
            Ok(()) => return Err(contract_error("unclaimed fixture member was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("unexpected fixture asset") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_symlinked_fixture_members() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("fixture");
        fs::create_dir(&fixture)?;
        let target = root.path().join("target.pl");
        fs::write(&target, "my $value = 1;\n")?;
        let link = fixture.join("linked.pl");
        if !create_file_symlink(&target, &link)? {
            return Ok(());
        }

        let error = match validate_fixture_members(&fixture) {
            Ok(()) => return Err(contract_error("fixture symlink was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("symbolic link") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_dangling_symlinked_fixture_members() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("fixture");
        fs::create_dir(&fixture)?;
        let link = fixture.join("dangling.pl");
        if !create_file_symlink(&root.path().join("missing.pl"), &link)? {
            return Ok(());
        }

        let error = match validate_fixture_members(&fixture) {
            Ok(()) => return Err(contract_error("dangling fixture symlink was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("symbolic link") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_nested_module_payload_symlinks() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("fixture");
        let nested = fixture.join("lib").join("nested");
        fs::create_dir_all(&nested)?;
        let target = root.path().join("Target.pm");
        fs::write(&target, "package Target;\n1;\n")?;
        let link = nested.join("Target.pm");
        if !create_file_symlink(&target, &link)? {
            return Ok(());
        }

        let error = match validate_fixture_members(&fixture) {
            Ok(()) => return Err(contract_error("nested module symlink was accepted").into()),
            Err(error) => error,
        };
        if !error.to_string().contains("symbolic link") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_reparse_fixture_directory() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let target = root.path().join("real_fixture");
        fs::create_dir(&target)?;
        let link = root.path().join("linked_fixture");
        if !create_directory_symlink(&target, &link)? {
            return Ok(());
        }

        let error = match fixture_directories(root.path()) {
            Ok(_) => {
                return Err(contract_error("Windows reparse fixture directory was accepted").into());
            }
            Err(error) => error,
        };
        if !error.to_string().contains("symbolic link") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }
}
