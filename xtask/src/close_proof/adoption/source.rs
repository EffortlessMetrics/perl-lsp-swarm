use super::super::{content_digest_hex, wire};
use super::{CallerExpectedSelection, CandidateMapping, MappingReport, not_proven};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const MAX_RESPONSE_BYTES: usize = 262_144;
pub(super) const READ_COUNT: usize = 7;

/// Arbitrary offline data, including recorded native responses, remains Simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationSources {
    pub comment_first: Vec<u8>,
    pub issue_first: Vec<u8>,
    pub policy_commit: Vec<u8>,
    pub template_metadata: Vec<u8>,
    pub template_bytes: Vec<u8>,
    pub issue_last: Vec<u8>,
    pub comment_last: Vec<u8>,
}

impl SimulationSources {
    pub fn from_json_str(raw: &str) -> Result<Self, super::super::CloseProofError> {
        wire::from_json_str(raw, "simulation_sources")
    }
    fn responses(&self) -> [&[u8]; READ_COUNT] {
        [
            &self.comment_first,
            &self.issue_first,
            &self.policy_commit,
            &self.template_metadata,
            &self.template_bytes,
            &self.issue_last,
            &self.comment_last,
        ]
    }
}

pub(super) struct ObservedSources {
    pub record: String,
    pub issue: String,
    pub native: bool,
}

pub(super) trait SourceReader {
    fn read(&mut self, endpoint: &str, raw: bool) -> Result<Vec<u8>, String>;
}

struct SimulationReader<'a> {
    responses: [&'a [u8]; READ_COUNT],
    next: usize,
}
impl SourceReader for SimulationReader<'_> {
    fn read(&mut self, _endpoint: &str, _raw: bool) -> Result<Vec<u8>, String> {
        let bytes = self
            .responses
            .get(self.next)
            .ok_or_else(|| "source-read count exceeded".to_string())?;
        self.next += 1;
        Ok(bytes.to_vec())
    }
}

fn exact_json(raw: &[u8]) -> Result<Value, String> {
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err("source response exceeds byte bound".into());
    }
    let text =
        std::str::from_utf8(raw).map_err(|_| "source response is not exact UTF8".to_string())?;
    wire::from_json_str(text, "source_response").map_err(|error| error.to_string())
}
fn string_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string source field {key}"))
}
fn body(value: &Value, digest: &str) -> Result<String, String> {
    let text = string_field(value, "body")?;
    if content_digest_hex(text.as_bytes()) != digest {
        return Err("exact source body digest changed".into());
    }
    Ok(text.into())
}
fn comment(raw: &[u8], selection: &CallerExpectedSelection) -> Result<String, String> {
    let value = exact_json(raw)?;
    let expected_issue =
        format!("https://api.github.com/repos/{}/issues/{}", selection.repository, selection.issue);
    let expected_comment = format!(
        "https://api.github.com/repos/{}/issues/comments/{}",
        selection.repository, selection.comment_id
    );
    if value.get("id").and_then(Value::as_u64) != Some(selection.comment_id)
        || string_field(&value, "issue_url")? != expected_issue
        || string_field(&value, "url")? != expected_comment
    {
        return Err("selected comment is bound to another native object".into());
    }
    body(&value, selection.record_digest)
}
fn issue(raw: &[u8], selection: &CallerExpectedSelection) -> Result<String, String> {
    let value = exact_json(raw)?;
    let expected =
        format!("https://api.github.com/repos/{}/issues/{}", selection.repository, selection.issue);
    if value.get("number").and_then(Value::as_u64) != Some(selection.issue)
        || string_field(&value, "url")? != expected
        || value.get("pull_request").is_some()
    {
        return Err("selected issue is bound to another native object or a PR".into());
    }
    body(&value, selection.issue_digest)
}

pub(super) fn observe(
    reader: &mut impl SourceReader,
    selection: &CallerExpectedSelection,
    native: bool,
) -> Result<ObservedSources, String> {
    selection.validate()?;
    let prefix = format!("repos/{}", selection.repository);
    let comment_endpoint = format!("{prefix}/issues/comments/{}", selection.comment_id);
    let issue_endpoint = format!("{prefix}/issues/{}", selection.issue);
    let template_endpoint = format!(
        "{prefix}/contents/.github/PULL_REQUEST_TEMPLATE.md?ref={}",
        selection.policy_commit
    );
    let record_first = comment(&reader.read(&comment_endpoint, false)?, selection)?;
    let issue_first = issue(&reader.read(&issue_endpoint, false)?, selection)?;
    let policy = exact_json(
        &reader.read(&format!("{prefix}/git/commits/{}", selection.policy_commit), false)?,
    )?;
    if string_field(&policy, "sha")? != selection.policy_commit {
        return Err("policy generation changed".into());
    }
    let metadata = exact_json(&reader.read(&template_endpoint, false)?)?;
    if string_field(&metadata, "sha")? != selection.template_blob
        || string_field(&metadata, "path")? != ".github/PULL_REQUEST_TEMPLATE.md"
        || string_field(&metadata, "type")? != "file"
    {
        return Err("pinned template identity changed".into());
    }
    let template = reader.read(&template_endpoint, true)?;
    if template.len() > MAX_RESPONSE_BYTES {
        return Err("template exceeds byte bound".into());
    }
    std::str::from_utf8(&template).map_err(|_| "template is not exact UTF8".to_string())?;
    if content_digest_hex(&template) != selection.template_digest {
        return Err("actual pinned template bytes changed".into());
    }
    let issue_last = issue(&reader.read(&issue_endpoint, false)?, selection)?;
    let record_last = comment(&reader.read(&comment_endpoint, false)?, selection)?;
    if record_first != record_last || issue_first != issue_last {
        return Err("first/last selected-source observation changed".into());
    }
    Ok(ObservedSources { record: record_first, issue: issue_first, native })
}

pub fn report_simulation(
    selection: &CallerExpectedSelection,
    candidate: &CandidateMapping,
    sources: &SimulationSources,
) -> MappingReport {
    let mut reader = SimulationReader { responses: sources.responses(), next: 0 };
    match observe(&mut reader, selection, false) {
        Ok(observed) => super::report(selection, candidate, observed),
        Err(error) => not_proven(error),
    }
}

/// Live retrieval is not qualified in this slice. Do not reroute denied credentials
/// or treat connector observations and offline fixtures as Rust native-reader proof.
/// A later bounded, host-bound adapter needs independent retrieval/lifecycle evidence.
pub fn report_live(
    _selection: &CallerExpectedSelection,
    _candidate: &CandidateMapping,
) -> MappingReport {
    not_proven("Native Rust retrieval adapter is not qualified; no live reads were attempted.")
}
