use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use perl_parser_pest::PureRustPerlParser;

use super::digest::sha256_digest;
use super::{
    Disposition, ExecutionMode, FixtureError, LoadedManifest, ResolvedFixture, Selection,
    load_manifest,
};

/// Current parser return for one fixture. This is an observation, not a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseObservation {
    /// `parse()` returned `Ok`. This is not correctness or acceptance.
    ReturnedOk {
        /// Digest of the current s-expression formatting.
        sexp_digest: String,
    },
    /// `parse()` returned `Err`. This is not a fixture-load failure.
    ReturnedErr {
        /// `Display` of the error value.
        message: String,
    },
    /// Fixture bytes are not UTF-8, so the current `&str` parse API cannot run.
    SourceNotUtf8,
}

impl ParseObservation {
    /// Render the observation class without promotion language.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::ReturnedOk { .. } => "parse-returned-ok",
            Self::ReturnedErr { .. } => "parse-returned-err",
            Self::SourceNotUtf8 => "source-not-utf8",
        }
    }
}

/// One fixture's recorded current observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentObservation {
    /// Stable fixture id.
    pub id: String,
    /// Train family.
    pub family: String,
    /// Exact source digest.
    pub source_digest: String,
    /// Catalog disposition. Seed rows are provisional.
    pub disposition: Disposition,
    /// Issue that owns the current observation.
    pub observation_owner: String,
    /// Issue that will own the final expected outcome, if any.
    pub expected_outcome_owner: Option<String>,
    /// Current parser return, never a `correct` label.
    pub parse: ParseObservation,
}

/// Observe one resolved fixture with the embedded Pest parser.
pub fn observe_with_embedded_parser(
    fixture: &ResolvedFixture,
) -> Result<CurrentObservation, FixtureError> {
    observe_resolved(fixture, |source| {
        let mut parser = PureRustPerlParser::new();
        match parser.parse(source) {
            Ok(ast) => Ok(parser.to_sexp(&ast)),
            Err(error) => Err(error.to_string()),
        }
    })
}

/// Observe one fixture with an injectable parse function.
///
/// A panic is a hard instrument failure, not a parser-rejection observation.
pub fn observe_resolved(
    fixture: &ResolvedFixture,
    parse: impl FnOnce(&str) -> Result<String, String>,
) -> Result<CurrentObservation, FixtureError> {
    let parse_observation = match std::str::from_utf8(&fixture.bytes) {
        Err(_) => ParseObservation::SourceNotUtf8,
        Ok(source) => match catch_unwind(AssertUnwindSafe(|| parse(source))) {
            Ok(Ok(sexp)) => {
                ParseObservation::ReturnedOk { sexp_digest: sha256_digest(sexp.as_bytes()) }
            }
            Ok(Err(message)) => ParseObservation::ReturnedErr { message },
            Err(payload) => {
                return Err(FixtureError::ParserPanic {
                    id: fixture.id.clone(),
                    message: panic_message(payload),
                });
            }
        },
    };
    Ok(CurrentObservation {
        id: fixture.id.clone(),
        family: fixture.family.clone(),
        source_digest: fixture.source_digest.clone(),
        disposition: fixture.disposition,
        observation_owner: fixture.observation_owner.clone(),
        expected_outcome_owner: fixture.expected_outcome_owner.clone(),
        parse: parse_observation,
    })
}

/// Load, select, and observe with the embedded parser.
///
/// This is the reusable entry point for later package-isolation and extraction
/// parity tests. It takes a package root rather than a workspace root.
pub fn run_embedded(
    package_root: &Path,
    selection: &Selection,
) -> Result<Vec<CurrentObservation>, FixtureError> {
    let loaded = load_manifest(package_root)?;
    run_embedded_loaded(&loaded, selection)
}

/// Observe an already-loaded catalog with the embedded parser.
pub fn run_embedded_loaded(
    loaded: &LoadedManifest,
    selection: &Selection,
) -> Result<Vec<CurrentObservation>, FixtureError> {
    loaded
        .select_with_mode(selection, Some(ExecutionMode::Embedded))?
        .into_iter()
        .map(observe_with_embedded_parser)
        .collect()
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
