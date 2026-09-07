//! Repository-level integrity and population gates for `test_corpus/gold`.
//!
//! Verify with:
//!
//! ```bash
//! cargo test -p perl-corpus --test gold_repository_contract
//! ```

use perl_corpus::byte_fidelity::{ByteFidelity, Encoding, NewlineStyle};
use perl_corpus::gold::{
    CompletionGoldExpected, DocumentSymbolGoldExpected, GoldAssertion, GoldExpected,
    GotoGoldExpected, HoverGoldExpected, RenameGoldExpected,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const MIN_FIXTURE_DIRECTORIES: usize = 36;

/// Floor on the number of members the byte-fidelity walk must classify, so a
/// walk that silently finds nothing cannot be mistaken for a clean corpus.
const MIN_GOLD_MEMBERS: usize = 90;

const SIDECAR_FLOORS: [(&str, usize); 7] = [
    ("expected.json", 28),
    ("expected_hover.json", 8),
    ("expected_goto.json", 3),
    ("expected_completion.json", 6),
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

/// A gold member whose bytes deliberately deviate from [`DEFAULT_BYTE_CLASS`].
///
/// Deviation is a reviewed, declared property, never an accident: the entry
/// pins the exact class the member is expected to have, and the member is
/// admitted only when `.gitattributes` disables git's text normalization for
/// its path. Without that protection git would be free to rewrite the very
/// bytes the entry claims are load-bearing.
#[derive(Debug, Clone, Copy)]
struct ByteExactDeviation {
    /// Repository-relative, `/`-separated path.
    path: &'static str,
    /// Terminator representation the member is expected to have.
    newline_style: NewlineStyle,
    /// Whether the member is expected to end with a terminator.
    final_newline: bool,
    /// Whether the member is expected to begin with a UTF-8 BOM.
    byte_order_mark: bool,
    /// Why these bytes are load-bearing.
    reason: &'static str,
}

/// The class every gold member has unless it is declared in
/// [`BYTE_EXACT_DEVIATIONS`]: LF terminators, a final newline, and no BOM.
const DEFAULT_BYTE_CLASS: (NewlineStyle, bool, bool) = (NewlineStyle::Lf, true, false);

/// Gold members that intentionally carry other bytes.
///
/// Empty today, and that emptiness is the contract: every member of
/// `test_corpus/gold` is currently LF-terminated UTF-8 without a BOM, and any
/// change to that — including one git makes on a contributor's behalf — has to
/// be declared here and protected in `.gitattributes` before it is admitted.
const BYTE_EXACT_DEVIATIONS: &[ByteExactDeviation] = &[];

fn contract_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| contract_error("perl-corpus must live under <workspace>/crates"))?;
    Ok(root.to_path_buf())
}

fn gold_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(workspace_root()?.join("test_corpus").join("gold"))
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
            if name == "README.md" {
                return Err(contract_error(format!(
                    "top-level README.md must be a regular file: {}",
                    path.display()
                ))
                .into());
            }
            directories.push(path);
            continue;
        }
        if name == "README.md" {
            if !file_type.is_file() {
                return Err(contract_error(format!(
                    "top-level README.md must be a regular file: {}",
                    path.display()
                ))
                .into());
            }
            continue;
        }
        return Err(contract_error(format!(
            "unexpected top-level gold corpus asset: {}",
            path.display()
        ))
        .into());
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
        if let GoldAssertion::DiagnosticPresent { byte_offset: Some(byte_offset), .. } = assertion
            && *byte_offset > source_len
        {
            return Err(contract_error(format!(
                "{} declares byte_offset {} past end of fixture length {}",
                path.display(),
                byte_offset,
                source_len
            ))
            .into());
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
            ModuleAssertion::Resolves {
                module,
                expected_suffix,
                use_line,
                use_col,
                consumers,
                rationale,
            } => {
                let _ = (expected_suffix, use_line, use_col);
                (module, consumers, rationale)
            }
            ModuleAssertion::NotResolved { module, use_line, use_col, consumers, rationale } => {
                let _ = (use_line, use_col);
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

/// Collect the repository-relative paths `.gitattributes` exempts from git's
/// text normalization.
///
/// Only literal path entries in the repository-root `.gitattributes` carrying
/// the `-text` attribute are recognised. Git's full pattern language, and its
/// per-directory `.gitattributes` files, are deliberately not reimplemented
/// here: a member whose exact bytes matter must be named literally at the root,
/// which is what the repository already does for the parser-accuracy span
/// fixtures. Any other spelling reads as unprotected, so the contract errs
/// toward rejecting a deviation rather than trusting one.
fn git_text_normalization_disabled(
    workspace_root: &Path,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let path = workspace_root.join(".gitattributes");
    let contents = fs::read_to_string(&path)
        .map_err(|error| contract_error(format!("reading {}: {error}", path.display())))?;
    Ok(parse_unnormalized_paths(&contents))
}

fn parse_unnormalized_paths(contents: &str) -> BTreeSet<String> {
    let mut protected = BTreeSet::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split_whitespace();
        let Some(pattern) = fields.next() else {
            continue;
        };
        // `-text` is the attribute that turns normalization off. `text`,
        // `text=auto`, and `eol=lf` all leave git free to rewrite the bytes.
        if fields.any(|attribute| attribute == "-text") {
            protected.insert(pattern.trim_start_matches("./").to_owned());
        }
    }

    protected
}

fn collect_gold_members(
    directory: &Path,
    members: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            return Err(contract_error(format!(
                "gold corpus contains a symbolic link: {}",
                path.display()
            ))
            .into());
        }
        if file_type.is_dir() {
            collect_gold_members(&path, members)?;
        } else if file_type.is_file() {
            members.push(path);
        } else {
            return Err(contract_error(format!(
                "gold corpus member is not a regular file: {}",
                path.display()
            ))
            .into());
        }
    }

    Ok(())
}

fn gold_members(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut members = Vec::new();
    collect_gold_members(root, &mut members)?;
    members.sort();
    Ok(members)
}

fn repository_relative(workspace_root: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    let relative = path
        .strip_prefix(workspace_root)
        .map_err(|_| contract_error(format!("{} is outside the workspace", path.display())))?;

    let mut rendered = String::new();
    for component in relative.components() {
        let Some(text) = component.as_os_str().to_str() else {
            return Err(contract_error(format!(
                "gold member path is not UTF-8: {}",
                path.display()
            ))
            .into());
        };
        if !rendered.is_empty() {
            rendered.push('/');
        }
        rendered.push_str(text);
    }

    Ok(rendered)
}

/// Judge one member's observed bytes against the declared contract.
///
/// Pure over its inputs so the rejection paths can be exercised directly,
/// without mutating a tracked fixture to observe a failure.
fn check_member_byte_fidelity(
    repository_path: &str,
    fidelity: ByteFidelity,
    deviations: &[ByteExactDeviation],
    protected_paths: &BTreeSet<String>,
) -> Result<(), String> {
    if let Encoding::InvalidUtf8 { valid_up_to } = fidelity.encoding {
        return Err(format!(
            "{repository_path} is not valid UTF-8: first undecodable byte at offset \
             {valid_up_to}. Gold members must decode exactly; a replacement character is \
             not an acceptable substitute for the original byte."
        ));
    }

    let Some(declared) = deviations.iter().find(|deviation| deviation.path == repository_path)
    else {
        let (newline_style, final_newline, byte_order_mark) = DEFAULT_BYTE_CLASS;
        if fidelity.newline_style != newline_style
            || fidelity.final_newline != final_newline
            || fidelity.byte_order_mark != byte_order_mark
        {
            return Err(format!(
                "{repository_path} has {fidelity}, but an undeclared gold member must be \
                 newlines={newline_style}, final newline={final_newline}, \
                 BOM={byte_order_mark}. Restore the bytes, or declare the deviation in \
                 BYTE_EXACT_DEVIATIONS and give the path a literal `-text` entry in \
                 .gitattributes."
            ));
        }
        return Ok(());
    };

    if !protected_paths.contains(repository_path) {
        return Err(format!(
            "{repository_path} is declared byte-exact ({}) but .gitattributes does not \
             disable text normalization for it. Add a literal `{repository_path} -text` \
             entry so git cannot rewrite the bytes the declaration depends on.",
            declared.reason
        ));
    }

    if fidelity.newline_style != declared.newline_style
        || fidelity.final_newline != declared.final_newline
        || fidelity.byte_order_mark != declared.byte_order_mark
    {
        return Err(format!(
            "{repository_path} is declared as newlines={}, final newline={}, BOM={} ({}), \
             but has {fidelity}. The declared bytes were rewritten.",
            declared.newline_style,
            declared.final_newline,
            declared.byte_order_mark,
            declared.reason
        ));
    }

    Ok(())
}

/// The first path declared more than once, if any.
///
/// Two entries for one member would let the first silently shadow the second,
/// so a duplicate is a contract error rather than a resolved conflict.
fn duplicate_declaration(deviations: &[ByteExactDeviation]) -> Option<&'static str> {
    let mut seen = BTreeSet::new();
    deviations.iter().find(|deviation| !seen.insert(deviation.path)).map(|deviation| deviation.path)
}

fn validate_gold_byte_fidelity(root: &Path) -> Result<usize, Box<dyn Error>> {
    if let Some(path) = duplicate_declaration(BYTE_EXACT_DEVIATIONS) {
        return Err(contract_error(format!(
            "BYTE_EXACT_DEVIATIONS declares {path} more than once; one member cannot hold \
             two byte classes"
        ))
        .into());
    }

    let workspace_root = workspace_root()?;
    let protected_paths = git_text_normalization_disabled(&workspace_root)?;
    let members = gold_members(root)?;

    let mut observed_paths = BTreeSet::new();
    for member in &members {
        let bytes = fs::read(member)
            .map_err(|error| contract_error(format!("reading {}: {error}", member.display())))?;
        let repository_path = repository_relative(&workspace_root, member)?;

        check_member_byte_fidelity(
            &repository_path,
            ByteFidelity::classify(&bytes),
            BYTE_EXACT_DEVIATIONS,
            &protected_paths,
        )
        .map_err(contract_error)?;

        observed_paths.insert(repository_path);
    }

    // A declaration that no longer names a real member is stale authority.
    for deviation in BYTE_EXACT_DEVIATIONS {
        if !observed_paths.contains(deviation.path) {
            return Err(contract_error(format!(
                "BYTE_EXACT_DEVIATIONS names {}, which is not a gold corpus member",
                deviation.path
            ))
            .into());
        }
    }

    Ok(members.len())
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

    // Byte fidelity gates the decoded views deliberately: every later step
    // reads members as `String`, so an undecodable member must be reported
    // here — with its path and the offending offset — rather than surfacing
    // downstream as an anonymous "stream did not contain valid UTF-8".
    let classified = validate_gold_byte_fidelity(&root)?;
    if classified < MIN_GOLD_MEMBERS {
        return Err(contract_error(format!(
            "byte-fidelity classification saw only {classified} gold members; floor is \
             {MIN_GOLD_MEMBERS}. A walk that finds nothing must not pass as a clean corpus."
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

    fn write_fixture_file(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
        fs::write(path, contents)?;
        Ok(())
    }

    fn validation_error<T>(result: Result<T, Box<dyn Error>>) -> Result<String, Box<dyn Error>> {
        match result {
            Ok(_) => Err(contract_error("invalid fixture was accepted").into()),
            Err(error) => Ok(error.to_string()),
        }
    }

    #[test]
    fn rejects_unknown_expected_sidecars() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        write_fixture_file(&directory.path().join("expected_future.json"), "{}")?;
        let error = validation_error(reject_unknown_sidecars(directory.path()))?;
        if !error.contains("unregistered gold sidecar") {
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

        let mut sidecar_counts = BTreeMap::new();
        let error = validation_error(validate_fixture_directory(&fixture, &mut sidecar_counts))?;
        if !error.contains("no recognized assertion sidecar") {
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
            let error = validation_error(validate_named_sidecar(&sidecar, "fixture"))?;
            if !error.contains(expected_message) {
                return Err(contract_error(format!("unexpected validation error: {error}")).into());
            }
        }
        Ok(())
    }

    #[test]
    fn diagnostics_are_closed_world_and_eof_is_a_valid_position() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected.json");

        write_fixture_file(
            &sidecar,
            r#"{"diagnostics":[{"assertion":"diagnostic_present","code":"PL001","byte_offset":1}]}"#,
        )?;
        validate_diagnostics_sidecar(&sidecar, 1)?;

        write_fixture_file(
            &sidecar,
            r#"{"diagnostics":[{"assertion":"diagnostic_present","code":"PL001","byte_offset":2}]}"#,
        )?;
        let error = validation_error(validate_diagnostics_sidecar(&sidecar, 1))?;
        if !error.contains("past end of fixture length") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }

        for invalid in [
            r#"{"diagnostics":[{"assertion":"no_diagnostics"}],"diagnotics":[]}"#,
            r#"{"diagnostics":[{"assertion":"no_diagnostic","code":"PL001","codde":"PL001"}]}"#,
        ] {
            write_fixture_file(&sidecar, invalid)?;
            let error = validation_error(validate_diagnostics_sidecar(&sidecar, 1))?;
            if !error.contains("unknown field") {
                return Err(contract_error(format!("unexpected validation error: {error}")).into());
            }
        }

        Ok(())
    }

    #[test]
    fn named_sidecars_reject_unknown_fields() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected_completion.json");
        let valid = r#"{"version":1,"fixture":"fixture","assertions":[{"kind":"completion_present","line":0,"character":0,"expected_label":"name","rationale":"known fields"}]}"#;
        write_fixture_file(&sidecar, valid)?;
        validate_named_sidecar(&sidecar, "fixture")?;

        let invalid = valid.replace(
            "\"rationale\":\"known fields\"",
            "\"rationale\":\"known fields\",\"unexpected_assertion\":true",
        );
        write_fixture_file(&sidecar, &invalid)?;
        let error = validation_error(validate_named_sidecar(&sidecar, "fixture"))?;
        if !error.contains("unexpected_assertion") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rename_schema_rejects_unexecutable_states_and_typos() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let sidecar = directory.path().join("expected_rename.json");
        let envelope = |assertion: &str| {
            format!(r#"{{"version":1,"fixture":"fixture","assertions":[{assertion}]}}"#)
        };

        let omitted = envelope(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values"}"#,
        );
        write_fixture_file(&sidecar, &omitted)?;
        validate_named_sidecar(&sidecar, "fixture")?;

        for invalid in [
            envelope(
                r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":null}"#,
            ),
            envelope(
                r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":[]}"#,
            ),
            envelope(
                r#"{"kind":"rename_null","line":4,"character":4,"new_name":"sum_values","expected_edits":[{"line":4,"character":4,"end_line":4,"end_character":19,"new_text":"sum_values"}]}"#,
            ),
            envelope(
                r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","min":1}"#,
            ),
            envelope(
                r#"{"kind":"rename_edit_count_at_least","min":0,"line":4,"character":4,"new_name":"sum_values"}"#,
            ),
            envelope(
                r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_editz":[]}"#,
            ),
            envelope(
                r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":[{"line":4,"character":4,"end_line":4,"end_character":19,"new_text":"sum_values","new_texxt":"sum_values"}]}"#,
            ),
        ] {
            write_fixture_file(&sidecar, &invalid)?;
            if validate_named_sidecar(&sidecar, "fixture").is_ok() {
                return Err(contract_error(format!(
                    "invalid rename assertion was accepted: {invalid}"
                ))
                .into());
            }
        }

        let count = envelope(
            r#"{"kind":"rename_edit_count_at_least","min":1,"line":4,"character":4,"new_name":"sum_values"}"#,
        );
        write_fixture_file(&sidecar, &count)?;
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

        let error = validation_error(validate_fixture_members(&fixture))?;
        if !error.contains("unexpected fixture asset") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[test]
    fn rejects_top_level_readme_directory() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        fs::create_dir(root.path().join("README.md"))?;

        let error = validation_error(fixture_directories(root.path()))?;
        if !error.contains("README.md must be a regular file") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_fixture_members() -> Result<(), Box<dyn Error>> {
        let root = tempdir()?;
        let fixture = root.path().join("fixture");
        fs::create_dir(&fixture)?;
        let target = root.path().join("target.pl");
        fs::write(&target, "my $value = 1;\n")?;
        std::os::unix::fs::symlink(&target, fixture.join("linked.pl"))?;

        let error = validation_error(validate_fixture_members(&fixture))?;
        if !error.contains("symbolic link") {
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
        if perl_tdd_support::symlink_test_decision().skip_visibly() {
            return Ok(());
        }
        if perl_tdd_support::try_create_dir_symlink(&target, &link)?.is_none() {
            return Ok(());
        }

        let error = validation_error(fixture_directories(root.path()))?;
        if !error.contains("symbolic link") {
            return Err(contract_error(format!("unexpected validation error: {error}")).into());
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Byte fidelity (#15011)
    //
    // The rejection paths are exercised against synthetic members so that
    // proving the contract bites never requires rewriting a tracked fixture.
    // -----------------------------------------------------------------------

    const LF_SOURCE: &[u8] = b"use strict;\nmy $x = 1;\n";

    fn protected(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn rejection(
        repository_path: &str,
        bytes: &[u8],
        deviations: &[ByteExactDeviation],
        protected_paths: &BTreeSet<String>,
    ) -> Result<String, Box<dyn Error>> {
        match check_member_byte_fidelity(
            repository_path,
            ByteFidelity::classify(bytes),
            deviations,
            protected_paths,
        ) {
            Ok(()) => Err(contract_error(format!(
                "{repository_path} was admitted, but the contract must reject it"
            ))
            .into()),
            Err(message) => Ok(message),
        }
    }

    #[test]
    fn an_ordinary_lf_member_is_admitted() -> Result<(), Box<dyn Error>> {
        check_member_byte_fidelity(
            "test_corpus/gold/hello_world/fixture.pl",
            ByteFidelity::classify(LF_SOURCE),
            BYTE_EXACT_DEVIATIONS,
            &BTreeSet::new(),
        )
        .map_err(contract_error)?;
        Ok(())
    }

    #[test]
    fn rejects_a_member_converted_to_crlf() -> Result<(), Box<dyn Error>> {
        let crlf = b"use strict;\r\nmy $x = 1;\r\n";
        let message = rejection(
            "test_corpus/gold/hello_world/fixture.pl",
            crlf,
            BYTE_EXACT_DEVIATIONS,
            &BTreeSet::new(),
        )?;
        assert!(
            message.contains("newlines=CRLF"),
            "message must name the observed class: {message}"
        );
        Ok(())
    }

    #[test]
    fn rejects_a_member_that_lost_or_gained_a_final_newline() -> Result<(), Box<dyn Error>> {
        let stripped = b"use strict;\nmy $x = 1;";
        let message = rejection(
            "test_corpus/gold/hello_world/fixture.pl",
            stripped,
            BYTE_EXACT_DEVIATIONS,
            &BTreeSet::new(),
        )?;
        assert!(
            message.contains("final newline=false"),
            "message must name the missing terminator: {message}"
        );
        Ok(())
    }

    #[test]
    fn rejects_a_member_that_gained_a_byte_order_mark() -> Result<(), Box<dyn Error>> {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(LF_SOURCE);

        let message = rejection(
            "test_corpus/gold/hello_world/fixture.pl",
            &with_bom,
            BYTE_EXACT_DEVIATIONS,
            &BTreeSet::new(),
        )?;
        assert!(message.contains("BOM=true"), "message must name the BOM: {message}");
        Ok(())
    }

    #[test]
    fn rejects_undecodable_bytes_by_offset_instead_of_replacing_them() -> Result<(), Box<dyn Error>>
    {
        let message = rejection(
            "test_corpus/gold/hello_world/fixture.pl",
            b"use strict;\n\xFFmy $x = 1;\n",
            BYTE_EXACT_DEVIATIONS,
            &BTreeSet::new(),
        )?;
        assert!(
            message.contains("not valid UTF-8") && message.contains("offset 12"),
            "message must name the undecodable offset: {message}"
        );
        Ok(())
    }

    #[test]
    fn rejects_a_declared_deviation_that_git_may_normalize() -> Result<(), Box<dyn Error>> {
        let declared = [ByteExactDeviation {
            path: "test_corpus/gold/crlf_positions/fixture.pl",
            newline_style: NewlineStyle::Crlf,
            final_newline: true,
            byte_order_mark: false,
            reason: "CRLF position fixture",
        }];

        let message = rejection(
            "test_corpus/gold/crlf_positions/fixture.pl",
            b"use strict;\r\nmy $x = 1;\r\n",
            &declared,
            // Nothing protects the path: git is free to rewrite the bytes.
            &BTreeSet::new(),
        )?;
        assert!(
            message.contains(".gitattributes"),
            "message must point at the missing git protection: {message}"
        );
        Ok(())
    }

    #[test]
    fn admits_a_declared_deviation_that_git_cannot_normalize() -> Result<(), Box<dyn Error>> {
        let declared = [ByteExactDeviation {
            path: "test_corpus/gold/crlf_positions/fixture.pl",
            newline_style: NewlineStyle::Crlf,
            final_newline: true,
            byte_order_mark: false,
            reason: "CRLF position fixture",
        }];

        check_member_byte_fidelity(
            "test_corpus/gold/crlf_positions/fixture.pl",
            ByteFidelity::classify(b"use strict;\r\nmy $x = 1;\r\n"),
            &declared,
            &protected(&["test_corpus/gold/crlf_positions/fixture.pl"]),
        )
        .map_err(contract_error)?;
        Ok(())
    }

    /// The scenario the whole contract exists for: a fixture declared CRLF
    /// whose bytes arrived as LF because something normalized them. Git
    /// protection alone cannot catch this; only comparing declared bytes to
    /// observed bytes can.
    #[test]
    fn rejects_a_declared_crlf_member_whose_bytes_arrived_as_lf() -> Result<(), Box<dyn Error>> {
        let declared = [ByteExactDeviation {
            path: "test_corpus/gold/crlf_positions/fixture.pl",
            newline_style: NewlineStyle::Crlf,
            final_newline: true,
            byte_order_mark: false,
            reason: "CRLF position fixture",
        }];

        let message = rejection(
            "test_corpus/gold/crlf_positions/fixture.pl",
            LF_SOURCE,
            &declared,
            &protected(&["test_corpus/gold/crlf_positions/fixture.pl"]),
        )?;
        assert!(
            message.contains("declared as newlines=CRLF") && message.contains("newlines=LF"),
            "message must contrast declared and observed bytes: {message}"
        );
        Ok(())
    }

    #[test]
    fn gitattributes_reader_separates_protected_paths_from_normalized_ones() {
        let protected_paths = parse_unnormalized_paths(
            "# comment\n\
             * text eol=lf\n\
             \n\
             crates/x/crlf.pl -text !eol whitespace=-blank-at-eol\n\
             crates/x/normal.pl text\n\
             *.png binary\n",
        );

        assert!(protected_paths.contains("crates/x/crlf.pl"));
        assert!(
            !protected_paths.contains("crates/x/normal.pl"),
            "a `text` entry leaves git free to normalize"
        );
        assert!(!protected_paths.contains("*"), "the default `* text eol=lf` is not protection");
        assert!(!protected_paths.contains("*.png"), "`binary` is not the attribute we read");
    }

    /// Positive and negative control against the repository's real
    /// `.gitattributes`, so the reader cannot drift from the file it reads.
    #[test]
    fn repository_gitattributes_protects_the_span_fixtures_and_not_the_gold_corpus()
    -> Result<(), Box<dyn Error>> {
        let protected_paths = git_text_normalization_disabled(&workspace_root()?)?;

        assert!(
            protected_paths.contains("crates/perl-corpus/fixtures/parser_accuracy/span_crlf.pl"),
            "the parser-accuracy CRLF fixture is byte-exact and must be protected"
        );
        assert!(
            !protected_paths.contains("test_corpus/gold/hello_world/fixture.pl"),
            "no gold member is exempt from normalization today; if one becomes exempt it \
             must also be declared in BYTE_EXACT_DEVIATIONS"
        );
        Ok(())
    }

    #[test]
    fn a_path_declared_twice_is_a_contract_error_not_a_silent_shadow() {
        let first = ByteExactDeviation {
            path: "test_corpus/gold/crlf_positions/fixture.pl",
            newline_style: NewlineStyle::Crlf,
            final_newline: true,
            byte_order_mark: false,
            reason: "CRLF position fixture",
        };
        let shadowing = ByteExactDeviation { newline_style: NewlineStyle::Lf, ..first };

        assert_eq!(duplicate_declaration(&[first]), None);
        assert_eq!(
            duplicate_declaration(&[first, shadowing]),
            Some("test_corpus/gold/crlf_positions/fixture.pl")
        );
    }

    #[test]
    fn every_gold_member_is_classified_and_none_is_skipped() -> Result<(), Box<dyn Error>> {
        let root = gold_root()?;
        let members = gold_members(&root)?;

        assert!(
            members.len() >= MIN_GOLD_MEMBERS,
            "gold corpus member count regressed to {}",
            members.len()
        );
        assert!(
            members.iter().any(|member| member.ends_with("fixture.pl")),
            "the walk must reach fixture sources"
        );
        assert!(
            members
                .iter()
                .any(|member| member.extension().is_some_and(|extension| extension == "pm")),
            "the walk must reach lib payload modules, not just top-level members"
        );
        Ok(())
    }
}
