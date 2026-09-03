use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Assertion type for gold corpus diagnostics expectations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "assertion", deny_unknown_fields)]
pub enum GoldAssertion {
    /// No diagnostics should be emitted for this fixture
    #[serde(rename = "no_diagnostics")]
    NoDiagnostics,

    /// A specific diagnostic should NOT be present
    #[serde(rename = "no_diagnostic")]
    NoDiagnostic { code: String },

    /// A diagnostic with the given code should be present
    #[serde(rename = "diagnostic_present")]
    DiagnosticPresent {
        code: String,
        #[serde(default)]
        byte_offset: Option<usize>,
        #[serde(default)]
        message_contains: Option<String>,
    },

    /// Expect exactly N diagnostics with the given code
    #[serde(rename = "diagnostic_count")]
    DiagnosticCount { code: String, count: usize },
}

/// Expected diagnostics for a gold corpus fixture
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldExpected {
    pub diagnostics: Vec<GoldAssertion>,
}

/// A gold corpus test fixture with expected results
#[derive(Debug, Clone)]
pub struct GoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub expected: GoldExpected,
}

/// Load a single gold fixture from a directory
///
/// Expects a directory with:
/// - `fixture.pl` — the Perl source code to test
/// - `expected.json` — JSON file with GoldExpected assertions
pub fn load_gold_fixture<P: AsRef<Path>>(
    dir: P,
) -> Result<GoldFixture, Box<dyn std::error::Error>> {
    let dir = dir.as_ref();
    let name = dir.file_name().ok_or("No directory name")?.to_string_lossy().to_string();

    let fixture_path = dir.join("fixture.pl");
    let expected_path = dir.join("expected.json");

    if !fixture_path.exists() {
        return Err(format!("fixture.pl not found in {}", dir.display()).into());
    }
    if !expected_path.exists() {
        return Err(format!("expected.json not found in {}", dir.display()).into());
    }

    let expected_json = fs::read_to_string(&expected_path)?;
    let expected: GoldExpected = serde_json::from_str(&expected_json)?;

    Ok(GoldFixture { name, fixture_path, expected })
}

/// Load all gold fixtures from a directory
///
/// Walks the directory and loads all subdirectories as fixtures.
pub fn load_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<GoldFixture>, Box<dyn std::error::Error>> {
    load_gold_fixtures_from(root)
}

/// Load all gold fixtures from a directory
///
/// Walks the directory and loads all subdirectories as fixtures.
pub fn load_gold_fixtures_from<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<GoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            match load_gold_fixture(&path) {
                Ok(fixture) => fixtures.push(fixture),
                Err(e) => {
                    // Use tracing instead of eprintln
                    tracing::warn!("Failed to load fixture from {}: {}", path.display(), e);
                }
            }
        }
    }

    // Sort by name for consistent ordering
    fixtures.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Editor Intelligence — Hover
// ---------------------------------------------------------------------------

/// Assertion kind for hover gold corpus entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum HoverAssertionKind {
    /// Response must have non-null, non-empty content
    HoverNonNull,
    /// Response must be null (no hover available)
    HoverNull,
    /// Response content must contain `needle` (case-sensitive)
    HoverContains { needle: String },
    /// Response content must NOT contain `needle`
    HoverAbsent { needle: String },
}

/// A single hover assertion at a given (line, character) position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverAssertion {
    #[serde(flatten)]
    pub kind: HoverAssertionKind,
    pub line: u32,
    pub character: u32,
    #[serde(default)]
    pub rationale: String,
}

/// On-disk representation of `expected_hover.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoverGoldExpected {
    pub version: u32,
    pub fixture: String,
    pub assertions: Vec<HoverAssertion>,
}

/// A hover gold fixture (fixture.pl + expected_hover.json)
#[derive(Debug, Clone)]
pub struct HoverGoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub hover_assertions: Vec<HoverAssertion>,
}

/// Load all hover gold fixtures from a directory.
///
/// Walks the directory and loads all subdirectories that contain both
/// `fixture.pl` and `expected_hover.json`. Directories that lack
/// `expected_hover.json` are silently skipped.
pub fn load_hover_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<HoverGoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let fixture_path = path.join("fixture.pl");
        let hover_path = path.join("expected_hover.json");

        if !fixture_path.exists() || !hover_path.exists() {
            continue; // not a hover fixture
        }

        let name = path.file_name().ok_or("No directory name")?.to_string_lossy().to_string();
        let json = fs::read_to_string(&hover_path)?;
        let expected: HoverGoldExpected = serde_json::from_str(&json)
            .map_err(|e| format!("Parsing {}: {e}", hover_path.display()))?;

        fixtures.push(HoverGoldFixture {
            name,
            fixture_path,
            hover_assertions: expected.assertions,
        });
    }

    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Editor Intelligence — Goto-Definition
// ---------------------------------------------------------------------------

/// Assertion kind for goto-definition gold corpus entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum GotoAssertionKind {
    /// Response must return at least one location
    GotoNonNull,
    /// Response must return no locations
    GotoNull,
    /// The first returned location must be on the given line
    GotoLine { expected_line: u32 },
}

/// A single goto-definition assertion at a given (line, character) position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotoAssertion {
    #[serde(flatten)]
    pub kind: GotoAssertionKind,
    pub line: u32,
    pub character: u32,
    #[serde(default)]
    pub rationale: String,
}

/// On-disk representation of `expected_goto.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GotoGoldExpected {
    pub version: u32,
    pub fixture: String,
    pub assertions: Vec<GotoAssertion>,
}

/// A goto-definition gold fixture (fixture.pl + expected_goto.json)
#[derive(Debug, Clone)]
pub struct GotoGoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub goto_assertions: Vec<GotoAssertion>,
}

/// Load all goto-definition gold fixtures from a directory.
///
/// Silently skips directories that lack `expected_goto.json`.
pub fn load_goto_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<GotoGoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let fixture_path = path.join("fixture.pl");
        let goto_path = path.join("expected_goto.json");

        if !fixture_path.exists() || !goto_path.exists() {
            continue;
        }

        let name = path.file_name().ok_or("No directory name")?.to_string_lossy().to_string();
        let json = fs::read_to_string(&goto_path)?;
        let expected: GotoGoldExpected = serde_json::from_str(&json)
            .map_err(|e| format!("Parsing {}: {e}", goto_path.display()))?;

        fixtures.push(GotoGoldFixture { name, fixture_path, goto_assertions: expected.assertions });
    }

    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Editor Intelligence — Completion
// ---------------------------------------------------------------------------

/// Assertion kind for completion gold corpus entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum CompletionAssertionKind {
    /// Completion list must not be empty
    CompletionNonEmpty,
    /// Item must be at position `[0]` in the list
    CompletionTop1 { expected_label: String },
    /// Item must be in positions `[0..4]`
    CompletionTop5 { expected_label: String },
    /// Item must appear anywhere in the list
    CompletionPresent { expected_label: String },
    /// Label must NOT appear in the list
    CompletionNoiseAbsent { forbidden_label: String },
}

/// A single completion assertion at a given (line, character) position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionAssertion {
    #[serde(flatten)]
    pub kind: CompletionAssertionKind,
    pub line: u32,
    pub character: u32,
    #[serde(default)]
    pub rationale: String,
}

/// On-disk representation of `expected_completion.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionGoldExpected {
    pub version: u32,
    pub fixture: String,
    pub assertions: Vec<CompletionAssertion>,
}

/// A completion gold fixture (fixture.pl + expected_completion.json)
#[derive(Debug, Clone)]
pub struct CompletionGoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub completion_assertions: Vec<CompletionAssertion>,
}

/// Load all completion gold fixtures from a directory.
///
/// Silently skips directories that lack `expected_completion.json`.
pub fn load_completion_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<CompletionGoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let fixture_path = path.join("fixture.pl");
        let completion_path = path.join("expected_completion.json");

        if !fixture_path.exists() || !completion_path.exists() {
            continue;
        }

        let name = path.file_name().ok_or("No directory name")?.to_string_lossy().to_string();
        let json = fs::read_to_string(&completion_path)?;
        let expected: CompletionGoldExpected = serde_json::from_str(&json)
            .map_err(|e| format!("Parsing {}: {e}", completion_path.display()))?;

        fixtures.push(CompletionGoldFixture {
            name,
            fixture_path,
            completion_assertions: expected.assertions,
        });
    }

    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Editor Intelligence — Document Symbols
// ---------------------------------------------------------------------------

/// Assertion kind for document-symbol gold corpus entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum DocumentSymbolAssertionKind {
    /// Symbols list must not be empty
    SymbolNonEmpty,
    /// Symbol with exact `name` must be present in returned symbols
    SymbolPresent { name: String },
    /// Symbol with exact `name` must not be present
    SymbolAbsent { name: String },
    /// Exactly `count` symbols should be returned
    SymbolCount { count: usize },
}

/// A single document-symbol assertion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbolAssertion {
    #[serde(flatten)]
    pub kind: DocumentSymbolAssertionKind,
    #[serde(default)]
    pub rationale: String,
}

/// On-disk representation of `expected_symbols.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSymbolGoldExpected {
    pub version: u32,
    pub fixture: String,
    pub assertions: Vec<DocumentSymbolAssertion>,
}

/// A document-symbol gold fixture (fixture.pl + expected_symbols.json)
#[derive(Debug, Clone)]
pub struct DocumentSymbolGoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub symbol_assertions: Vec<DocumentSymbolAssertion>,
}

/// Load all document-symbol gold fixtures from a directory.
///
/// Silently skips directories that lack `expected_symbols.json`.
pub fn load_document_symbol_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<DocumentSymbolGoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let fixture_path = path.join("fixture.pl");
        let symbols_path = path.join("expected_symbols.json");

        if !fixture_path.exists() || !symbols_path.exists() {
            continue;
        }

        let name = path.file_name().ok_or("No directory name")?.to_string_lossy().to_string();
        let json = fs::read_to_string(&symbols_path)?;
        let expected: DocumentSymbolGoldExpected = serde_json::from_str(&json)
            .map_err(|e| format!("Parsing {}: {e}", symbols_path.display()))?;

        fixtures.push(DocumentSymbolGoldFixture {
            name,
            fixture_path,
            symbol_assertions: expected.assertions,
        });
    }

    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

// ---------------------------------------------------------------------------
// Editor Intelligence — Rename
// ---------------------------------------------------------------------------

/// Assertion kind for rename gold corpus entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RenameAssertionKind {
    /// Rename must return a non-null WorkspaceEdit with at least one edit
    RenameSucceeds,
    /// Rename must return null or an error (symbol is not renamable)
    RenameNull,
    /// WorkspaceEdit must contain at least `min` text edits across all files
    RenameEditCountAtLeast { min: usize },
}

/// A single rename assertion at a given (line, character) position
#[derive(Debug, Clone, Serialize)]
pub struct RenameAssertion {
    #[serde(flatten)]
    pub kind: RenameAssertionKind,
    pub line: u32,
    pub character: u32,
    pub new_name: String,
    /// Exact edits expected when this assertion exercises a concrete rename.
    /// Omission preserves the count-only contract of older fixtures. A present
    /// array must contain at least one exact edit. `null` and empty arrays are
    /// rejected so they cannot silently weaken or contradict success modes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expected_edits"
    )]
    pub expected_edits: Option<Vec<RenameExpectedEdit>>,
    #[serde(default)]
    pub rationale: String,
}

impl<'de> Deserialize<'de> for RenameAssertion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(RenameAssertionVisitor)
    }
}

struct RenameAssertionVisitor;

impl<'de> Visitor<'de> for RenameAssertionVisitor {
    type Value = RenameAssertion;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a rename assertion object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        const FIELDS: &[&str] =
            &["kind", "line", "character", "new_name", "min", "expected_edits", "rationale"];
        let mut kind: Option<String> = None;
        let mut line: Option<u32> = None;
        let mut character: Option<u32> = None;
        let mut new_name: Option<String> = None;
        let mut min: Option<usize> = None;
        let mut expected_edits: Option<Vec<RenameExpectedEdit>> = None;
        let mut expected_edits_seen = false;
        let mut rationale: Option<String> = None;

        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "kind" => {
                    if kind.is_some() {
                        return Err(de::Error::duplicate_field("kind"));
                    }
                    kind = Some(map.next_value()?);
                }
                "line" => {
                    if line.is_some() {
                        return Err(de::Error::duplicate_field("line"));
                    }
                    line = Some(map.next_value()?);
                }
                "character" => {
                    if character.is_some() {
                        return Err(de::Error::duplicate_field("character"));
                    }
                    character = Some(map.next_value()?);
                }
                "new_name" => {
                    if new_name.is_some() {
                        return Err(de::Error::duplicate_field("new_name"));
                    }
                    new_name = Some(map.next_value()?);
                }
                "min" => {
                    if min.is_some() {
                        return Err(de::Error::duplicate_field("min"));
                    }
                    min = Some(map.next_value()?);
                }
                "expected_edits" => {
                    if expected_edits_seen {
                        return Err(de::Error::duplicate_field("expected_edits"));
                    }
                    expected_edits_seen = true;
                    expected_edits = map.next_value()?;
                    if expected_edits.is_none() {
                        return Err(de::Error::custom(
                            "expected_edits must be omitted or an array; null is not supported",
                        ));
                    }
                }
                "rationale" => {
                    if rationale.is_some() {
                        return Err(de::Error::duplicate_field("rationale"));
                    }
                    rationale = Some(map.next_value()?);
                }
                _ => return Err(de::Error::unknown_field(&field, FIELDS)),
            }
        }

        let kind_name = kind.ok_or_else(|| de::Error::missing_field("kind"))?;
        let kind = match kind_name.as_str() {
            "rename_succeeds" => {
                if min.is_some() {
                    return Err(de::Error::custom(
                        "min is only supported for rename_edit_count_at_least assertions",
                    ));
                }
                RenameAssertionKind::RenameSucceeds
            }
            "rename_null" => {
                if min.is_some() {
                    return Err(de::Error::custom(
                        "min is only supported for rename_edit_count_at_least assertions",
                    ));
                }
                RenameAssertionKind::RenameNull
            }
            "rename_edit_count_at_least" => {
                let min = min.ok_or_else(|| de::Error::missing_field("min"))?;
                if min == 0 {
                    return Err(de::Error::custom(
                        "rename_edit_count_at_least requires min to be at least 1",
                    ));
                }
                RenameAssertionKind::RenameEditCountAtLeast { min }
            }
            _ => {
                return Err(de::Error::unknown_variant(
                    &kind_name,
                    &["rename_succeeds", "rename_null", "rename_edit_count_at_least"],
                ));
            }
        };
        if expected_edits.as_ref().is_some_and(Vec::is_empty) {
            return Err(de::Error::custom(
                "expected_edits must be omitted or contain at least one edit",
            ));
        }
        if matches!(&kind, RenameAssertionKind::RenameNull) && expected_edits.is_some() {
            return Err(de::Error::custom(
                "expected_edits is only supported for rename assertions that inspect edits",
            ));
        }

        Ok(RenameAssertion {
            kind,
            line: line.ok_or_else(|| de::Error::missing_field("line"))?,
            character: character.ok_or_else(|| de::Error::missing_field("character"))?,
            new_name: new_name.ok_or_else(|| de::Error::missing_field("new_name"))?,
            expected_edits,
            rationale: rationale.unwrap_or_default(),
        })
    }
}

/// One expected text edit in a rename workspace edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameExpectedEdit {
    /// URI of the edited document.  Omitted values retain the legacy
    /// same-document shorthand and are resolved by the scorecard harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

/// On-disk representation of `expected_rename.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameGoldExpected {
    pub version: u32,
    pub fixture: String,
    pub assertions: Vec<RenameAssertion>,
}

/// A rename gold fixture (fixture.pl + expected_rename.json)
#[derive(Debug, Clone)]
pub struct RenameGoldFixture {
    pub name: String,
    pub fixture_path: PathBuf,
    pub rename_assertions: Vec<RenameAssertion>,
}

/// Load all rename gold fixtures from a directory.
///
/// Silently skips directories that lack `expected_rename.json`.
pub fn load_rename_gold_fixtures<P: AsRef<Path>>(
    root: P,
) -> Result<Vec<RenameGoldFixture>, Box<dyn std::error::Error>> {
    let root = root.as_ref();
    let mut fixtures = Vec::new();

    if !root.exists() {
        return Err(format!("Gold fixtures directory not found: {}", root.display()).into());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let fixture_path = path.join("fixture.pl");
        let rename_path = path.join("expected_rename.json");

        if !fixture_path.exists() || !rename_path.exists() {
            continue;
        }

        let name = path.file_name().ok_or("No directory name")?.to_string_lossy().to_string();
        let json = fs::read_to_string(&rename_path)?;
        let expected: RenameGoldExpected = serde_json::from_str(&json)
            .map_err(|e| format!("Parsing {}: {e}", rename_path.display()))?;

        fixtures.push(RenameGoldFixture {
            name,
            fixture_path,
            rename_assertions: expected.assertions,
        });
    }

    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gold_assertion_deserialization() -> Result<(), serde_json::Error> {
        let json = r#"{"assertion": "no_diagnostics"}"#;
        let assertion: GoldAssertion = serde_json::from_str(json)?;
        assert!(matches!(assertion, GoldAssertion::NoDiagnostics));
        Ok(())
    }

    #[test]
    fn test_gold_assertion_diagnostic_present() -> Result<(), serde_json::Error> {
        let json = r#"{"assertion": "diagnostic_present", "code": "PL100", "byte_offset": 24}"#;
        let assertion: GoldAssertion = serde_json::from_str(json)?;
        assert!(
            matches!(
                &assertion,
                GoldAssertion::DiagnosticPresent {
                    code,
                    byte_offset: Some(24),
                    ..
                } if code == "PL100"
            ),
            "Expected DiagnosticPresent variant with code PL100 and byte_offset 24"
        );
        Ok(())
    }

    #[test]
    fn test_gold_expected_deserialization() -> Result<(), serde_json::Error> {
        let json = r#"{"diagnostics": [{"assertion": "no_diagnostics"}]}"#;
        let expected: GoldExpected = serde_json::from_str(json)?;
        assert_eq!(expected.diagnostics.len(), 1);
        Ok(())
    }

    #[test]
    fn diagnostics_schema_rejects_unknown_envelope_and_assertion_fields() {
        let unknown_envelope =
            r#"{"diagnostics":[{"assertion":"no_diagnostics"}],"diagnotics":[]}"#;
        assert!(
            serde_json::from_str::<GoldExpected>(unknown_envelope).is_err(),
            "unknown diagnostics envelope fields must fail closed"
        );

        let unknown_assertion =
            r#"{"diagnostics":[{"assertion":"no_diagnostic","code":"PL100","codde":"PL100"}]}"#;
        assert!(
            serde_json::from_str::<GoldExpected>(unknown_assertion).is_err(),
            "unknown diagnostics assertion fields must fail closed"
        );
    }

    #[test]
    fn test_malformed_json_returns_error() {
        // Malformed expected.json must produce a serde error, not panic
        let bad_json = r#"{"diagnostics": [{"assertion": "unknown_variant"}]}"#;
        let result: Result<GoldExpected, _> = serde_json::from_str(bad_json);
        assert!(result.is_err(), "unknown assertion variant should fail to deserialize");
    }

    #[test]
    fn test_empty_diagnostics_array_deserializes() -> Result<(), serde_json::Error> {
        // empty array is valid JSON but the harness will catch it as vacuous
        let json = r#"{"diagnostics": []}"#;
        let expected: GoldExpected = serde_json::from_str(json)?;
        assert_eq!(expected.diagnostics.len(), 0);
        Ok(())
    }

    #[test]
    fn test_no_diagnostic_deserialization() -> Result<(), serde_json::Error> {
        let json = r#"{"assertion": "no_diagnostic", "code": "PL100"}"#;
        let assertion: GoldAssertion = serde_json::from_str(json)?;
        assert!(
            matches!(&assertion, GoldAssertion::NoDiagnostic { code } if code == "PL100"),
            "Expected NoDiagnostic variant with code PL100"
        );
        Ok(())
    }

    #[test]
    fn test_diagnostic_count_deserialization() -> Result<(), serde_json::Error> {
        let json = r#"{"assertion": "diagnostic_count", "code": "PL001", "count": 3}"#;
        let assertion: GoldAssertion = serde_json::from_str(json)?;
        assert!(
            matches!(&assertion, GoldAssertion::DiagnosticCount { code, count: 3 } if code == "PL001"),
            "Expected DiagnosticCount variant with code PL001 and count 3"
        );
        Ok(())
    }

    #[test]
    fn rename_expected_edits_round_trips_omission_and_rejects_invalid_states()
    -> Result<(), Box<dyn std::error::Error>> {
        let omitted: RenameAssertion = serde_json::from_str(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values"}"#,
        )?;
        if omitted.expected_edits.is_some() {
            return Err("omitted expected_edits must remain count-only mode".into());
        }
        let serialized = serde_json::to_value(&omitted)?;
        if serialized.get("expected_edits").is_some() {
            return Err("omitted expected_edits must serialize as omission".into());
        }
        let round_tripped: RenameAssertion = serde_json::from_value(serialized)?;
        if round_tripped.expected_edits.is_some() {
            return Err("omitted expected_edits must round-trip as count-only mode".into());
        }

        let explicit_null = serde_json::from_str::<RenameAssertion>(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":null}"#,
        );
        if explicit_null.is_ok() {
            return Err("explicit null expected_edits must fail closed".into());
        }

        let explicit_empty = serde_json::from_str::<RenameAssertion>(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":[]}"#,
        );
        if explicit_empty.is_ok() {
            return Err("explicit empty expected_edits must fail closed".into());
        }

        let rename_null_with_edits = r#"{"kind":"rename_null","line":4,"character":4,"new_name":"sum_values","expected_edits":[{"line":4,"character":4,"end_line":4,"end_character":19,"new_text":"sum_values"}]}"#;
        if serde_json::from_str::<RenameAssertion>(rename_null_with_edits).is_ok() {
            return Err("expected_edits for rename_null must fail closed".into());
        }

        let zero_minimum = serde_json::from_str::<RenameAssertion>(
            r#"{"kind":"rename_edit_count_at_least","min":0,"line":4,"character":4,"new_name":"sum_values"}"#,
        );
        if zero_minimum.is_ok() {
            return Err("rename_edit_count_at_least min=0 must fail closed".into());
        }

        Ok(())
    }

    #[test]
    fn rename_schema_rejects_nested_typos_and_variant_mismatched_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let nested_typo = r#"{
            "version": 1,
            "fixture": "fixture",
            "assertions": [{
                "kind": "rename_succeeds",
                "line": 4,
                "character": 4,
                "new_name": "sum_values",
                "expected_edits": [{
                    "line": 4,
                    "character": 4,
                    "end_line": 4,
                    "end_character": 19,
                    "new_text": "sum_values",
                    "new_texxt": "sum_values"
                }]
            }]
        }"#;
        if serde_json::from_str::<RenameGoldExpected>(nested_typo).is_ok() {
            return Err("unknown nested expected edit field was accepted".into());
        }

        for kind in ["rename_succeeds", "rename_null"] {
            let mismatched = format!(
                r#"{{"kind":"{kind}","line":4,"character":4,"new_name":"sum_values","min":1}}"#
            );
            if serde_json::from_str::<RenameAssertion>(&mismatched).is_ok() {
                return Err(format!("min was accepted for {kind}").into());
            }
        }

        let count = serde_json::from_str::<RenameAssertion>(
            r#"{"kind":"rename_edit_count_at_least","min":1,"line":4,"character":4,"new_name":"sum_values"}"#,
        )?;
        if !matches!(count.kind, RenameAssertionKind::RenameEditCountAtLeast { min: 1 }) {
            return Err("min was not retained for rename_edit_count_at_least".into());
        }

        Ok(())
    }

    #[test]
    fn rename_schema_rejects_duplicate_fields_before_validation() {
        let cases = [
            ("kind", r#""kind":"rename_null""#),
            ("line", r#""line":4,"line":5"#),
            ("character", r#""character":4,"character":5"#),
            ("new_name", r#""new_name":"one","new_name":"two""#),
            ("min", r#""min":1,"min":2"#),
            (
                "expected_edits",
                r#""expected_edits":[{"line":1,"character":1,"end_line":1,"end_character":2,"new_text":"x"}],"expected_edits":[{"line":1,"character":1,"end_line":1,"end_character":2,"new_text":"y"}]"#,
            ),
            ("rationale", r#""rationale":"one","rationale":"two""#),
        ];
        for (field, duplicate) in cases {
            let json = format!(
                r#"{{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values",{duplicate}}}"#
            );
            assert!(
                serde_json::from_str::<RenameAssertion>(&json).is_err(),
                "duplicate {field} field was accepted: {json}"
            );
        }
    }
}
