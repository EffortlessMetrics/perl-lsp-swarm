//! Test-only exact, fail-closed external-command double (#15139).
//!
//! This is fixture infrastructure, not a production process supervisor. A
//! mismatched, extra, unused, or stale invocation fails the fixture and never
//! resolves a live executable from `PATH`.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// One observed invocation, recorded with only the fields the fixture admits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedExternalCommand {
    pub program: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub stdin: Option<String>,
}

impl ObservedExternalCommand {
    pub fn new(program: impl Into<String>, argv: Vec<String>) -> Self {
        Self {
            program: command_identity(&program.into()),
            argv,
            cwd: None,
            env: BTreeMap::new(),
            stdin: None,
        }
    }

    pub fn canonical(&self) -> String {
        canonical_command(&self.program, &self.argv)
    }
}

/// How stdin is compared when an expectation names it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StdinContract {
    #[default]
    Ignored,
    Empty,
    Exact(String),
}

/// Terminal result returned when an expectation matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandResult {
    pub fn stdout(stdout: impl Into<String>) -> Self {
        Self { stdout: stdout.into(), stderr: String::new(), exit_code: 0 }
    }

    pub fn error(stderr: impl Into<String>) -> Self {
        Self { stdout: String::new(), stderr: stderr.into(), exit_code: 1 }
    }
}

/// Adapter-local supported `--json` field catalog plus the requested set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFieldContract {
    pub supported: BTreeSet<String>,
    pub requested: BTreeSet<String>,
    pub admitted_payload: Option<BTreeSet<String>>,
}

impl JsonFieldContract {
    pub fn exact_supported(fields: &str) -> Self {
        let requested = json_field_set(fields);
        Self { supported: requested.clone(), requested, admitted_payload: None }
    }
}

/// One expected invocation. Relaxations (set-valued `--json`, alternatives,
/// ignored env keys, optional calls) must be explicit on this object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedExternalCommand {
    pub program: String,
    pub argv: Vec<String>,
    pub json_flag_is_set: bool,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub stdin: StdinContract,
    pub times: usize,
    pub result: CommandResult,
    pub json_fields: Option<JsonFieldContract>,
}

impl ExpectedExternalCommand {
    pub fn exact(program: impl Into<String>, argv: Vec<String>) -> Self {
        Self {
            program: command_identity(&program.into()),
            argv,
            json_flag_is_set: false,
            cwd: None,
            env: BTreeMap::new(),
            stdin: StdinContract::Ignored,
            times: 1,
            result: CommandResult::stdout(""),
            json_fields: None,
        }
    }

    pub fn from_tokens(tokens: &[&str]) -> Option<Self> {
        let (program, rest) = tokens.split_first()?;
        if program.is_empty() {
            return None;
        }
        Some(Self::exact(*program, rest.iter().map(|arg| (*arg).to_string()).collect()))
    }

    pub fn stdout(mut self, stdout: impl Into<String>) -> Self {
        self.result = CommandResult::stdout(stdout);
        self
    }

    pub fn error(mut self, stderr: impl Into<String>) -> Self {
        self.result = CommandResult::error(stderr);
        self
    }

    pub fn times(mut self, times: usize) -> Self {
        self.times = times;
        self
    }

    /// Domain fixtures that cache a canned surface, rather than proving call
    /// count, must opt into reuse explicitly.
    pub fn reusable(mut self) -> Self {
        self.times = usize::MAX;
        self
    }

    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env_binding(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn stdin_exact(mut self, stdin: impl Into<String>) -> Self {
        self.stdin = StdinContract::Exact(stdin.into());
        self
    }

    /// Treat the value of `--json` as an order-insensitive field set. Ordinary
    /// argv remains ordered.
    pub fn json_fields_are_a_set(mut self) -> Self {
        self.json_flag_is_set = true;
        self
    }

    pub fn json_contract(mut self, contract: JsonFieldContract) -> Self {
        self.json_flag_is_set = true;
        self.json_fields = Some(contract);
        self
    }

    pub fn identity(&self) -> String {
        canonical_command(&self.program, &self.argv)
    }

    fn key_starts_with(&self, selector: &str) -> bool {
        self.identity() == selector || self.identity().starts_with(&format!("{selector} "))
    }
}

/// Outcome of presenting one observation to the fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalCommandFixtureResult {
    Matched(CommandResult),
    Mismatch(FixtureMismatch),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    UnexpectedInvocation,
    ArgumentMismatch,
    WrongCwd,
    WrongEnv,
    WrongStdin,
    UnsupportedJsonField,
    JsonFieldSetMismatch,
    UnexpectedPayloadField,
    UnusedExpectation,
    ExtraInvocation,
    CountMismatch,
    StaleSubject,
    FixtureFailed,
}

impl MismatchKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::UnexpectedInvocation => "unexpected invocation",
            Self::ArgumentMismatch => "argument mismatch",
            Self::WrongCwd => "working directory mismatch",
            Self::WrongEnv => "selected environment mismatch",
            Self::WrongStdin => "stdin mismatch",
            Self::UnsupportedJsonField => "unsupported json field",
            Self::JsonFieldSetMismatch => "json field set mismatch",
            Self::UnexpectedPayloadField => "fixture payload field was not requested",
            Self::UnusedExpectation => "unused expectation",
            Self::ExtraInvocation => "extra invocation",
            Self::CountMismatch => "call count mismatch",
            Self::StaleSubject => "stale subject payload",
            Self::FixtureFailed => "fixture already failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureMismatch {
    pub kind: MismatchKind,
    pub observed: Option<ObservedExternalCommand>,
    pub expected_identity: Option<String>,
    pub selected_env: BTreeSet<String>,
}

impl FixtureMismatch {
    fn new(
        kind: MismatchKind,
        observed: Option<ObservedExternalCommand>,
        expected_identity: Option<String>,
    ) -> Self {
        Self { kind, observed, expected_identity, selected_env: BTreeSet::new() }
    }

    fn with_selected_env(mut self, selected_env: BTreeSet<String>) -> Self {
        self.selected_env = selected_env;
        self
    }
}

impl fmt::Display for FixtureMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "external-command fixture mismatch: {}", self.kind.as_str())?;
        if let Some(observed) = &self.observed {
            write!(f, "\nobserved: {}", redact_command(observed, &self.selected_env))?;
        }
        if let Some(expected) = &self.expected_identity {
            write!(f, "\nexpected: {expected}")?;
        }
        Ok(())
    }
}

struct ExpectationSlot {
    spec: ExpectedExternalCommand,
    remaining: usize,
}

#[derive(Default)]
struct FixtureInner {
    slots: Vec<ExpectationSlot>,
    observations: Vec<ObservedExternalCommand>,
    mismatch: Option<FixtureMismatch>,
}

/// In-process exact command double. Unknown commands never fall through.
#[derive(Default)]
pub struct ExternalCommandFixture {
    inner: RefCell<FixtureInner>,
}

impl ExternalCommandFixture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn expect(self, spec: ExpectedExternalCommand) -> Self {
        {
            let mut inner = self.inner.borrow_mut();
            let remaining = spec.times;
            inner.slots.push(ExpectationSlot { spec, remaining });
        }
        self
    }

    /// Replace the result of every registered expectation whose identity
    /// starts with `selector`. If none match, insert `selector` as an exact
    /// expectation. Observation matching stays exact; this is only a test
    /// authoring override against a previously registered full argv.
    pub fn on_selector(self, selector: &str, result: CommandResult) -> Self {
        {
            let mut inner = self.inner.borrow_mut();
            let mut replaced = false;
            for slot in &mut inner.slots {
                if slot.spec.key_starts_with(selector) {
                    slot.spec.result = result.clone();
                    replaced = true;
                }
            }
            if !replaced {
                let tokens: Vec<&str> = selector.split_whitespace().collect();
                if let Some(mut spec) = ExpectedExternalCommand::from_tokens(&tokens) {
                    spec.result = result;
                    let remaining = spec.times;
                    inner.slots.push(ExpectationSlot { spec, remaining });
                }
            }
        }
        self
    }

    pub fn observe(&self, observed: ObservedExternalCommand) -> ExternalCommandFixtureResult {
        let mut inner = self.inner.borrow_mut();
        if let Some(existing) = inner.mismatch.clone() {
            return ExternalCommandFixtureResult::Mismatch(existing);
        }
        inner.observations.push(observed.clone());

        let mut found = None;
        let mut argv_mismatch = None;
        let mut constraint_mismatch: Option<(MismatchKind, String, BTreeSet<String>)> = None;
        for (index, slot) in inner.slots.iter().enumerate() {
            if slot.remaining == 0 {
                continue;
            }
            match observation_fits(&slot.spec, &observed) {
                SlotFit::Match => {
                    found = Some(index);
                    break;
                }
                SlotFit::SameProgramWrongArgv => {
                    argv_mismatch.get_or_insert(slot.spec.identity());
                }
                SlotFit::WrongCwd => {
                    constraint_mismatch.get_or_insert((
                        MismatchKind::WrongCwd,
                        slot.spec.identity(),
                        BTreeSet::new(),
                    ));
                }
                SlotFit::WrongEnv => {
                    constraint_mismatch.get_or_insert((
                        MismatchKind::WrongEnv,
                        slot.spec.identity(),
                        slot.spec.env.keys().cloned().collect(),
                    ));
                }
                SlotFit::WrongStdin => {
                    constraint_mismatch.get_or_insert((
                        MismatchKind::WrongStdin,
                        slot.spec.identity(),
                        BTreeSet::new(),
                    ));
                }
                SlotFit::UnsupportedJsonField => {
                    constraint_mismatch.get_or_insert((
                        MismatchKind::UnsupportedJsonField,
                        slot.spec.identity(),
                        BTreeSet::new(),
                    ));
                }
                SlotFit::JsonFieldSetMismatch => {
                    constraint_mismatch.get_or_insert((
                        MismatchKind::JsonFieldSetMismatch,
                        slot.spec.identity(),
                        BTreeSet::new(),
                    ));
                }
                SlotFit::No => {}
            }
        }

        let Some(index) = found else {
            let mismatch =
                if let Some((kind, expected_identity, selected_env)) = constraint_mismatch {
                    FixtureMismatch::new(kind, Some(observed), Some(expected_identity))
                        .with_selected_env(selected_env)
                } else if let Some(expected_identity) = argv_mismatch {
                    FixtureMismatch::new(
                        MismatchKind::ArgumentMismatch,
                        Some(observed),
                        Some(expected_identity),
                    )
                } else {
                    FixtureMismatch::new(MismatchKind::UnexpectedInvocation, Some(observed), None)
                };
            inner.mismatch = Some(mismatch.clone());
            return ExternalCommandFixtureResult::Mismatch(mismatch);
        };

        if let Some(mismatch) = payload_field_mismatch(&inner.slots[index].spec, &observed) {
            inner.mismatch = Some(mismatch.clone());
            return ExternalCommandFixtureResult::Mismatch(mismatch);
        }

        inner.slots[index].remaining = inner.slots[index].remaining.saturating_sub(1);
        ExternalCommandFixtureResult::Matched(inner.slots[index].spec.result.clone())
    }

    pub fn observations(&self) -> Vec<ObservedExternalCommand> {
        self.inner.borrow().observations.clone()
    }

    pub fn mismatch(&self) -> Option<FixtureMismatch> {
        self.inner.borrow().mismatch.clone()
    }

    /// Fail if any finite expectation still has remaining calls, if a
    /// reusable expectation was never observed, or if a prior observation
    /// already mismatched.
    #[allow(clippy::result_large_err)]
    pub fn assert_exhausted(&self) -> Result<(), FixtureMismatch> {
        let inner = self.inner.borrow();
        if let Some(mismatch) = inner.mismatch.clone() {
            return Err(mismatch);
        }
        let unused: Vec<String> = inner
            .slots
            .iter()
            .filter(|slot| slot_is_unconsumed(slot))
            .map(|slot| slot.spec.identity())
            .collect();
        if unused.is_empty() {
            return Ok(());
        }
        Err(FixtureMismatch::new(MismatchKind::UnusedExpectation, None, unused.into_iter().next()))
    }

    pub fn slots_for_shim(&self) -> Vec<ExpectedExternalCommand> {
        self.inner.borrow().slots.iter().map(|slot| slot.spec.clone()).collect()
    }
}

fn slot_is_unconsumed(slot: &ExpectationSlot) -> bool {
    slot.remaining == slot.spec.times || (slot.remaining > 0 && slot.spec.times != usize::MAX)
}

enum SlotFit {
    Match,
    SameProgramWrongArgv,
    WrongCwd,
    WrongEnv,
    WrongStdin,
    UnsupportedJsonField,
    JsonFieldSetMismatch,
    No,
}

fn observation_fits(
    expected: &ExpectedExternalCommand,
    observed: &ObservedExternalCommand,
) -> SlotFit {
    if expected.program != observed.program {
        return SlotFit::No;
    }
    if let Some(kind) = json_slot_kind(expected, observed) {
        return kind;
    }
    if !argv_matches(expected, &observed.argv) {
        return SlotFit::SameProgramWrongArgv;
    }
    if let Some(cwd) = &expected.cwd
        && observed.cwd.as_deref() != Some(cwd.as_str())
    {
        return SlotFit::WrongCwd;
    }
    for (key, value) in &expected.env {
        if observed.env.get(key) != Some(value) {
            return SlotFit::WrongEnv;
        }
    }
    match &expected.stdin {
        StdinContract::Ignored => {}
        StdinContract::Empty => {
            if !observed.stdin.as_deref().unwrap_or("").is_empty() {
                return SlotFit::WrongStdin;
            }
        }
        StdinContract::Exact(expected_stdin) => {
            if observed.stdin.as_deref() != Some(expected_stdin.as_str()) {
                return SlotFit::WrongStdin;
            }
        }
    }
    SlotFit::Match
}

fn argv_matches(expected: &ExpectedExternalCommand, observed: &[String]) -> bool {
    if expected.argv.len() != observed.len() {
        return false;
    }
    let mut index = 0;
    while index < expected.argv.len() {
        if expected.json_flag_is_set
            && expected.argv[index] == "--json"
            && observed[index] == "--json"
        {
            let Some(expected_fields) = expected.argv.get(index + 1) else {
                return false;
            };
            let Some(observed_fields) = observed.get(index + 1) else {
                return false;
            };
            if json_field_set(expected_fields) != json_field_set(observed_fields) {
                return false;
            }
            index += 2;
            continue;
        }
        if expected.argv[index] != observed[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn json_slot_kind(
    expected: &ExpectedExternalCommand,
    observed: &ObservedExternalCommand,
) -> Option<SlotFit> {
    let contract = expected.json_fields.as_ref()?;
    if !command_shape_matches(&expected.argv, &observed.argv) {
        return None;
    }
    let requested = requested_json_fields(&observed.argv)?;
    if requested.iter().any(|field| !contract.supported.contains(field)) {
        return Some(SlotFit::UnsupportedJsonField);
    }
    if requested != contract.requested {
        return Some(SlotFit::JsonFieldSetMismatch);
    }
    None
}

fn command_shape_matches(expected: &[String], observed: &[String]) -> bool {
    if expected.len() != observed.len() {
        return false;
    }
    let mut index = 0;
    while index < expected.len() {
        if expected[index] == "--json" && observed[index] == "--json" {
            if index + 1 >= expected.len() || index + 1 >= observed.len() {
                return false;
            }
            index += 2;
            continue;
        }
        if expected[index] != observed[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn payload_field_mismatch(
    expected: &ExpectedExternalCommand,
    observed: &ObservedExternalCommand,
) -> Option<FixtureMismatch> {
    let contract = expected.json_fields.as_ref()?;
    let stdout = &expected.result.stdout;
    if stdout.trim().is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return None;
    };
    let allowed = contract.admitted_payload.as_ref().unwrap_or(&contract.requested);
    let extra = extra_payload_fields(&value, allowed);
    if extra.is_empty() {
        return None;
    }
    Some(FixtureMismatch::new(
        MismatchKind::UnexpectedPayloadField,
        Some(observed.clone()),
        Some(format!("unexpected payload fields {}", extra.join(","))),
    ))
}

fn extra_payload_fields(value: &serde_json::Value, allowed: &BTreeSet<String>) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => {
            map.keys().filter(|key| !allowed.contains(*key)).cloned().collect()
        }
        serde_json::Value::Array(items) => {
            let mut extra = BTreeSet::new();
            for item in items {
                extra.extend(extra_payload_fields(item, allowed));
            }
            extra.into_iter().collect()
        }
        _ => Vec::new(),
    }
}

pub fn json_field_set(fields: &str) -> BTreeSet<String> {
    fields.split(',').map(str::trim).filter(|field| !field.is_empty()).map(str::to_string).collect()
}

pub fn requested_json_fields(argv: &[String]) -> Option<BTreeSet<String>> {
    argv.windows(2).find_map(|pair| match pair {
        [flag, fields] if flag == "--json" => Some(json_field_set(fields)),
        _ => None,
    })
}

pub fn command_identity(program: &str) -> String {
    Path::new(program).file_name().and_then(|name| name.to_str()).unwrap_or(program).to_string()
}

pub fn canonical_command(program: &str, argv: &[String]) -> String {
    let mut tokens = Vec::with_capacity(argv.len() + 1);
    tokens.push(command_identity(program));
    tokens.extend(argv.iter().cloned());
    tokens.join(" ")
}

fn join_fields(fields: &BTreeSet<String>) -> String {
    fields.iter().cloned().collect::<Vec<_>>().join(",")
}

fn redact_command(observed: &ObservedExternalCommand, selected_env: &BTreeSet<String>) -> String {
    let mut rendered = observed.canonical();
    let env = selected_env
        .iter()
        .filter_map(|key| {
            let value = observed.env.get(key)?;
            if is_secret_key(key) {
                Some(format!("{key}=<redacted>"))
            } else {
                Some(format!("{key}={value}"))
            }
        })
        .collect::<Vec<_>>();
    if !env.is_empty() {
        rendered.push_str(" env[");
        rendered.push_str(&env.join(","));
        rendered.push(']');
    }
    if let Some(cwd) = &observed.cwd {
        rendered.push_str(" cwd=");
        rendered.push_str(cwd);
    }
    rendered
}

fn is_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "AUTHORIZATION", "PASSWD"]
        .iter()
        .any(|marker| upper.contains(marker))
        || (upper.contains("KEY") && !upper.contains("KEYRING"))
}

/// Parse a whitespace-separated selector into program + argv.
pub fn parse_selector(selector: &str) -> Option<(String, Vec<String>)> {
    let mut tokens = selector.split_whitespace().map(str::to_string);
    let program = tokens.next()?;
    Some((program, tokens.collect()))
}

#[cfg(unix)]
mod hermetic {
    use super::{
        ExpectedExternalCommand, ExternalCommandFixture, ObservedExternalCommand, command_identity,
    };
    use std::ffi::OsString;
    use std::fs;
    use std::io::{self, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const SHIM_SOURCE: &str = r#"#!/usr/bin/env bash
set -euo pipefail
dir="${EXTERNAL_COMMAND_DOUBLE_DIR:?}"
program=$(basename -- "$0")
mkdir -p "$dir/observed"
n=0
if [ -f "$dir/obs_count" ]; then
  n=$(cat "$dir/obs_count")
fi
n=$((n + 1))
printf '%s\n' "$n" > "$dir/obs_count"
{
  printf '%s\0' "$program"
  printf '%s\0' "$@"
} > "$dir/observed/$n.argv"
matched=
for slot in "$dir"/expected/*; do
  [ -d "$slot" ] || continue
  remaining=$(cat "$slot/remaining")
  if [ "$remaining" -eq 0 ]; then
    continue
  fi
  if cmp -s "$dir/observed/$n.argv" "$slot/argv"; then
    if [ "$remaining" -gt 0 ]; then
      remaining=$((remaining - 1))
      printf '%s\n' "$remaining" > "$slot/remaining"
    fi
    matched=1
    if [ -f "$slot/stdout" ]; then
      cat "$slot/stdout"
    fi
    exit "$(cat "$slot/exit")"
  fi
done
printf 'external-command fixture mismatch: unexpected invocation\n' > "$dir/mismatch"
printf 'observed: %s\n' "$program $*" >> "$dir/mismatch"
exit 99
"#;

    const SENTINEL_SOURCE: &str = r#"#!/usr/bin/env bash
set -euo pipefail
printf 'hit\n' > "${EXTERNAL_COMMAND_DOUBLE_SENTINEL:?}"
exit 98
"#;

    /// Isolated PATH for one intercepted executable name.
    pub struct HermeticCommandEnv {
        _temp: tempfile::TempDir,
        pub bin_dir: PathBuf,
        pub sentinel_dir: PathBuf,
        pub sentinel_marker: PathBuf,
        pub fixture_dir: PathBuf,
        passthrough: Vec<PathBuf>,
    }

    impl HermeticCommandEnv {
        pub fn install(program: &str, fixture: &ExternalCommandFixture) -> io::Result<Self> {
            let temp = tempfile::tempdir()?;
            let bin_dir = temp.path().join("bin");
            let sentinel_dir = temp.path().join("sentinel");
            let fixture_dir = temp.path().join("fixture");
            fs::create_dir(&bin_dir)?;
            fs::create_dir(&sentinel_dir)?;
            fs::create_dir(&fixture_dir)?;
            fs::create_dir(fixture_dir.join("expected"))?;
            fs::create_dir(fixture_dir.join("observed"))?;
            write_executable(&bin_dir.join(program), SHIM_SOURCE)?;
            write_executable(&sentinel_dir.join(program), SENTINEL_SOURCE)?;
            for (index, spec) in fixture.slots_for_shim().iter().enumerate() {
                write_slot(&fixture_dir.join("expected").join(index.to_string()), spec)?;
            }
            let sentinel_marker = fixture_dir.join("sentinel.marker");
            Ok(Self {
                _temp: temp,
                bin_dir,
                sentinel_dir,
                sentinel_marker,
                fixture_dir,
                passthrough: vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")],
            })
        }

        pub fn path_env(&self) -> io::Result<OsString> {
            let mut entries = vec![self.bin_dir.clone(), self.sentinel_dir.clone()];
            entries.extend(self.passthrough.iter().cloned());
            std::env::join_paths(entries).map_err(|error| io::Error::other(error.to_string()))
        }

        pub fn apply(&self, command: &mut Command) -> io::Result<()> {
            command.env("PATH", self.path_env()?);
            command.env("EXTERNAL_COMMAND_DOUBLE_DIR", &self.fixture_dir);
            command.env("EXTERNAL_COMMAND_DOUBLE_SENTINEL", &self.sentinel_marker);
            Ok(())
        }

        pub fn run_bash(
            &self,
            script: &str,
            extra_env: &[(&str, &str)],
        ) -> io::Result<std::process::Output> {
            let mut command = Command::new("/bin/bash");
            command.arg("-c").arg(script);
            self.apply(&mut command)?;
            for (key, value) in extra_env {
                command.env(key, value);
            }
            command.output()
        }

        pub fn sentinel_hit(&self) -> bool {
            self.sentinel_marker.exists()
        }

        pub fn mismatch_text(&self) -> Option<String> {
            fs::read_to_string(self.fixture_dir.join("mismatch")).ok()
        }

        pub fn observations(&self) -> io::Result<Vec<ObservedExternalCommand>> {
            let count_path = self.fixture_dir.join("obs_count");
            if !count_path.exists() {
                return Ok(Vec::new());
            }
            let count: usize = fs::read_to_string(count_path)?
                .trim()
                .parse()
                .map_err(|error| io::Error::other(format!("obs_count: {error}")))?;
            let mut observations = Vec::new();
            for index in 1..=count {
                let bytes =
                    fs::read(self.fixture_dir.join("observed").join(format!("{index}.argv")))?;
                let mut tokens = decode_argv_records(&bytes);
                if tokens.is_empty() {
                    continue;
                }
                let program = tokens.remove(0);
                observations.push(ObservedExternalCommand::new(program, tokens));
            }
            Ok(observations)
        }
    }

    fn encode_argv_records(program: &str, argv: &[String]) -> Vec<u8> {
        let mut records = Vec::new();
        for token in std::iter::once(program).chain(argv.iter().map(String::as_str)) {
            records.extend_from_slice(token.as_bytes());
            records.push(0);
        }
        records
    }

    fn decode_argv_records(bytes: &[u8]) -> Vec<String> {
        let mut tokens: Vec<String> = bytes
            .split(|byte| *byte == 0)
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        if tokens.last().is_some_and(String::is_empty) {
            tokens.pop();
        }
        tokens
    }

    fn write_slot(dir: &Path, spec: &ExpectedExternalCommand) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(
            dir.join("argv"),
            encode_argv_records(&command_identity(&spec.program), &spec.argv),
        )?;
        fs::write(dir.join("stdout"), &spec.result.stdout)?;
        fs::write(dir.join("exit"), format!("{}\n", spec.result.exit_code))?;
        let remaining =
            if spec.times == usize::MAX { "-1".to_string() } else { spec.times.to_string() };
        fs::write(dir.join("remaining"), format!("{remaining}\n"))?;
        Ok(())
    }

    fn write_executable(path: &Path, source: &str) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        file.write_all(source.as_bytes())?;
        drop(file);
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
    }
}

#[cfg(unix)]
#[allow(unused_imports)]
pub use hermetic::HermeticCommandEnv;
