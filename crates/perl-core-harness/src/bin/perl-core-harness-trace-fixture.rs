#![expect(
    clippy::print_stderr,
    reason = "The fixture emulates one instrumented upstream runner process: its stdout bytes ARE the discovery subject and stderr carries its failure diagnostics."
)]

//! Hermetic instrumented-runner fixture for the effective-invocation capture
//! route (#12285).
//!
//! The binary stands in for the prepared tree's host Perl executing one
//! EXACTLY PATCHED `t/TEST`: it verifies the artifact bytes in its working
//! directory measure the instrumented digest the capture route recorded
//! (proving the supervised process consumed the patched disposable copy, not
//! the ordinary runner), walks its selectors the way upstream `t/TEST`
//! `_find_tests` does, and — at the child-invocation decision seam the way
//! `_scan_test` does — emits one #12284 row frame per selected member to the
//! private trace channel file, then one terminal frame carrying its own
//! observed completion.
//!
//! The stream header is deliberately NOT written here: the header is
//! ProcessPlan-owned channel framing (session, parent nonce, parent receipt
//! digest), and the terminal integrity digest binds this process's row bytes
//! alone.
//!
//! Drift modes — selected through a `.trace-fixture-mode` marker in the
//! working directory — let the suite prove the capture route types honest
//! dispositions instead of guessing.

use perl_core_harness::invocation_trace::model::{
    CapturePoint, EffectiveInvocationField, EffectiveInvocationFields, ScriptRole, TaintMode,
    TestInitClass, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION, Utf8Switch,
};
use perl_core_harness::invocation_trace::{RunnerScheduling, SourceForm};
use perl_core_harness::observed_discovery::{EnvironmentIdentity, ProcessCompletion};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

/// Drift-mode marker read from the fixture tree's working directory.
const MODE_MARKER: &str = ".trace-fixture-mode";
/// Readiness marker written once the hang mode has flushed every stream, so
/// the suite can order supervision strictly after the evidence exists.
const READY_MARKER: &str = ".trace-fixture-ready";
/// Trace channel filename provided by the capture route (relative to cwd).
const TRACE_FILE_ENV: &str = "PERL_CORE_HARNESS_TRACE_FILE";
/// Trace session identity provided by the capture route.
const TRACE_SESSION_ENV: &str = "PERL_CORE_HARNESS_TRACE_SESSION";
/// Expected instrumented artifact digest the process must verify.
const TRACE_ARTIFACT_ENV: &str = "PERL_CORE_HARNESS_TRACE_ARTIFACT_SHA256";
/// Traced target identity provided by the capture route.
const TRACE_TARGET_ENV: &str = "PERL_CORE_HARNESS_TRACE_TARGET";
/// Instrumentation subject identity provided by the capture route.
const TRACE_INSTRUMENTATION_ENV: &str = "PERL_CORE_HARNESS_TRACE_INSTRUMENTATION";

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    if let Some(status) = emulate(&argv) {
        std::process::exit(status);
    }
    eprintln!("usage: perl-core-harness-trace-fixture TEST --dumptests <selector arguments>");
    std::process::exit(64);
}

/// Emulate one instrumented upstream `t/TEST --dumptests` invocation. Returns
/// the exit status, or `None` when the argv is not a discovery invocation.
fn emulate(argv: &[String]) -> Option<i32> {
    if argv.first().map(String::as_str) != Some("TEST") || !argv.iter().any(|a| a == "--dumptests")
    {
        return None;
    }
    let selectors = argv
        .iter()
        .skip(1)
        .filter(|argument| **argument != "--dumptests")
        .filter(|argument| !argument.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();
    match run_mode(&selectors) {
        Ok(status) => Some(status),
        Err(error) => {
            eprintln!("trace-fixture: {error}");
            Some(70)
        }
    }
}

/// One classified invocation decision, exactly as the upstream seam would
/// have determined it. Classification lives inside this stand-in process; the
/// capture route must never recompute it.
struct Decision {
    member: String,
    role: ScriptRole,
    include_roots: Vec<String>,
    interpreter_switches: Vec<String>,
    taint_mode: TaintMode,
    utf8: Utf8Switch,
    test_init: TestInitClass,
    run_cwd: String,
    return_directory: String,
    script_arguments: Vec<String>,
}

fn classify(member: &str) -> Decision {
    let family = member.strip_prefix("t/").and_then(|rest| rest.split('/').next()).unwrap_or("");
    let role = match family {
        "base" => ScriptRole::Base,
        "comp" => ScriptRole::Comp,
        "run" => ScriptRole::Run,
        _ => ScriptRole::Other,
    };
    let include_roots = match family {
        "base" => vec!["../lib".to_string(), "../t/lib".to_string()],
        "comp" => vec!["../lib".to_string(), "../cpan".to_string()],
        _ => vec!["../lib".to_string()],
    };
    let taint_mode = if member.contains("taintT") {
        TaintMode::TaintMode
    } else if member.contains("taintt") {
        TaintMode::TaintWarnings
    } else {
        TaintMode::None
    };
    let utf8 = if member.contains("utf8") { Utf8Switch::Utf8 } else { Utf8Switch::None };
    let test_init = if member.contains("init_u2t") {
        TestInitClass::U2t
    } else if member.contains("init_u2") {
        TestInitClass::U2
    } else if member.contains("init_u1") {
        TestInitClass::U1
    } else if member.contains("init_a") {
        TestInitClass::A
    } else if member.contains("init_nc") {
        TestInitClass::Nc
    } else {
        TestInitClass::Standard
    };
    let chdir = member.contains("chdir");
    let run_cwd = if chdir { format!("t/{family}") } else { "t".to_string() };
    // Upstream returns to the runner's own `t` directory after every chdir.
    let return_directory = "t".to_string();
    let mut interpreter_switches =
        include_roots.iter().map(|root| format!("-I{root}")).collect::<Vec<_>>();
    match taint_mode {
        TaintMode::TaintMode => interpreter_switches.push("-T".to_string()),
        TaintMode::TaintWarnings => interpreter_switches.push("-t".to_string()),
        TaintMode::None => {}
    }
    let script_arguments = if member.contains("with_args") {
        vec!["--flag".to_string(), "value".to_string()]
    } else {
        Vec::new()
    };
    Decision {
        member: member.to_string(),
        role,
        include_roots,
        interpreter_switches,
        taint_mode,
        utf8,
        test_init,
        run_cwd,
        return_directory,
        script_arguments,
    }
}

/// Behavior-bearing environment of one child invocation as decided at the
/// seam: the capture baseline plus the trace channel identity the instrument
/// itself observed, plus the per-invocation capability switches.
fn invocation_environment(utf8: Utf8Switch) -> EnvironmentIdentity {
    let mut variables = BTreeMap::new();
    for (key, value) in env::vars() {
        if key == "LC_ALL" || key.starts_with("PERL_CORE_HARNESS_TRACE_") {
            variables.insert(key, value);
        }
    }
    if utf8 == Utf8Switch::Utf8 {
        variables.insert("PERL_UNICODE".to_string(), "1".to_string());
    }
    let mut canonical = String::new();
    for (key, value) in &variables {
        canonical.push_str(key);
        canonical.push('=');
        canonical.push_str(value);
        canonical.push('\n');
    }
    EnvironmentIdentity { variables, sha256: sha_hex(canonical.as_bytes()) }
}

fn observed_fields(decision: &Decision) -> EffectiveInvocationFields {
    EffectiveInvocationFields {
        member_identity: EffectiveInvocationField::Observed { value: decision.member.clone() },
        source_form: EffectiveInvocationField::Observed { value: SourceForm::DotT },
        script_path: EffectiveInvocationField::Observed { value: decision.member.clone() },
        script_role: EffectiveInvocationField::Observed { value: decision.role },
        run_cwd: EffectiveInvocationField::Observed { value: decision.run_cwd.clone() },
        return_directory: EffectiveInvocationField::Observed {
            value: decision.return_directory.clone(),
        },
        interpreter_switches: EffectiveInvocationField::Observed {
            value: decision.interpreter_switches.clone(),
        },
        include_roots: EffectiveInvocationField::Observed { value: decision.include_roots.clone() },
        test_init: EffectiveInvocationField::Observed { value: decision.test_init },
        taint_mode: EffectiveInvocationField::Observed { value: decision.taint_mode },
        utf8_mode: EffectiveInvocationField::Observed { value: decision.utf8 },
        wrapper_arguments: EffectiveInvocationField::Observed { value: Vec::new() },
        script_arguments: EffectiveInvocationField::Observed {
            value: decision.script_arguments.clone(),
        },
        environment: EffectiveInvocationField::Observed {
            value: invocation_environment(decision.utf8),
        },
        scheduling: EffectiveInvocationField::Observed { value: RunnerScheduling::default() },
        capture_point: EffectiveInvocationField::Observed {
            value: CapturePoint::InvocationDecision,
        },
        upstream_operation: EffectiveInvocationField::Observed {
            value: "t/TEST runtests invocation decision".to_string(),
        },
    }
}

/// Wire row frame mirroring the strict decoder's row vocabulary exactly.
#[derive(serde::Serialize)]
struct WireRow {
    frame: &'static str,
    trace_session_id: String,
    sequence: u32,
    row_id: String,
    member: String,
    runner: &'static str,
    target_id: String,
    variant_target_id: Option<String>,
    instrumentation_id: String,
    fields: EffectiveInvocationFields,
}

/// Wire terminal frame mirroring the strict decoder's terminal vocabulary.
#[derive(serde::Serialize)]
struct WireTerminal {
    frame: &'static str,
    trace_session_id: String,
    row_count: u32,
    integrity_sha256: String,
    completion: ProcessCompletion,
}

struct Channel {
    rows: Vec<u8>,
    row_count: u32,
    path: std::path::PathBuf,
    session: String,
}

impl Channel {
    fn emit_row(
        &mut self,
        decision: &Decision,
        sequence: u32,
        fields: EffectiveInvocationFields,
    ) -> io::Result<()> {
        let frame = WireRow {
            frame: "row",
            trace_session_id: self.session.clone(),
            sequence,
            row_id: format!("trace-row-{sequence}"),
            member: decision.member.clone(),
            runner: "test",
            target_id: env::var(TRACE_TARGET_ENV).unwrap_or_default(),
            variant_target_id: None,
            instrumentation_id: env::var(TRACE_INSTRUMENTATION_ENV).unwrap_or_default(),
            fields,
        };
        let mut line =
            serde_json::to_vec(&frame).map_err(|error| io::Error::other(error.to_string()))?;
        line.push(b'\n');
        self.rows.extend_from_slice(&line);
        self.row_count += 1;
        Ok(())
    }

    fn write_trace(&self) -> io::Result<()> {
        let mut file = fs::File::create(&self.path)?;
        file.write_all(&self.rows)?;
        file.flush()
    }

    fn write_trace_with_terminal(&self, completion: ProcessCompletion) -> io::Result<()> {
        let mut file = fs::File::create(&self.path)?;
        file.write_all(&self.rows)?;
        let terminal = WireTerminal {
            frame: "terminal",
            trace_session_id: self.session.clone(),
            row_count: self.row_count,
            integrity_sha256: sha_hex(&self.rows),
            completion,
        };
        let mut line =
            serde_json::to_vec(&terminal).map_err(|error| io::Error::other(error.to_string()))?;
        line.push(b'\n');
        file.write_all(&line)?;
        file.flush()
    }
}

fn run_mode(selectors: &[String]) -> io::Result<i32> {
    // The supervised process must prove it consumed the instrumented
    // disposable copy, never the ordinary runner artifact.
    let expected_digest = env::var(TRACE_ARTIFACT_ENV).unwrap_or_default();
    let artifact_bytes = fs::read("TEST")?;
    let measured = sha_hex(&artifact_bytes);
    if measured != expected_digest {
        eprintln!(
            "trace-fixture: working-dir TEST measures {measured} but the capture route pinned \
             instrumented subject {expected_digest}"
        );
        return Ok(66);
    }
    let trace_path = env::var(TRACE_FILE_ENV).unwrap_or_default();
    if trace_path.is_empty() {
        return Err(io::Error::other("trace channel file was not provided"));
    }
    let session = env::var(TRACE_SESSION_ENV).unwrap_or_default();
    if session.is_empty() {
        return Err(io::Error::other("trace session identity was not provided"));
    }
    let mode = fs::read_to_string(MODE_MARKER)
        .map(|raw| raw.trim().to_string())
        .unwrap_or_else(|_| "clean".to_string());

    let mut members = selected_rows(selectors)?;
    if mode == "extra_member" {
        // Outside the base selector root: the parent membership keeps the
        // typed out-of-target disposition instead of accepting it.
        members.push("t/comp/foreign_extra.t".to_string());
    }
    members.sort();
    members.dedup();

    let mut channel = Channel {
        rows: Vec::new(),
        row_count: 0,
        path: Path::new(&trace_path).to_path_buf(),
        session: session.clone(),
    };

    match mode.as_str() {
        "hang" => {
            // A mid-run observation: rows are emitted to both streams and
            // flushed (signalled through the readiness marker) before the
            // process parks, so supervision types the terminal state from
            // real retained evidence and the trace keeps its un-terminated
            // row prefix.
            write_rows(&members)?;
            for (sequence, member) in members.iter().enumerate() {
                let decision = classify(member);
                channel.emit_row(&decision, sequence as u32, observed_fields(&decision))?;
            }
            channel.write_trace()?;
            let _ = fs::write(READY_MARKER, "ready\n");
            park_forever();
            Ok(0)
        }
        "empty" => {
            channel.write_trace_with_terminal(ProcessCompletion::ExitStatus { code: 0 })?;
            Ok(0)
        }
        "nonzero" => {
            write_rows(&members)?;
            emit_all(&mut channel, &members, |_| None)?;
            channel.write_trace_with_terminal(ProcessCompletion::ExitStatus { code: 7 })?;
            Ok(7)
        }
        "lying_terminal" => {
            // The instrument claims a clean terminal while the process exits
            // nonzero: the capture route must type the disagreement instead
            // of completing on the trace's word alone.
            write_rows(&members)?;
            emit_all(&mut channel, &members, |_| None)?;
            channel.write_trace_with_terminal(ProcessCompletion::ExitStatus { code: 0 })?;
            Ok(7)
        }
        "truncated" => {
            // Rows without a terminal frame: truncation is never a clean
            // observation.
            write_rows(&members)?;
            emit_all(&mut channel, &members, |_| None)?;
            channel.write_trace()?;
            Ok(0)
        }
        "contaminate" => {
            // Trace-frame bytes deliberately entering ordinary stdout: the
            // parent discovery stream fails the independent-transport law.
            write_rows(&members)?;
            emit_all(&mut channel, &members, |_| None)?;
            channel.write_trace_with_terminal(ProcessCompletion::ExitStatus { code: 0 })?;
            let contamination = format!(
                "{{\"frame\":\"header\",\"schema_version\":\"{UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION}\"}}\n"
            );
            let mut stdout = io::stdout();
            stdout.write_all(contamination.as_bytes())?;
            stdout.flush()?;
            Ok(0)
        }
        _ => emit_default(&mut channel, &members, &mode),
    }
}

/// Default emission with the discriminating per-row drift mutations.
fn emit_default(channel: &mut Channel, members: &[String], mode: &str) -> io::Result<i32> {
    write_rows(members)?;
    let foreign_session = session_foreign_to(&channel.session);
    emit_all(channel, members, |position| {
        let mutation = match mode {
            "missing_field" if position == 0 => Some(FieldMutation::MissingEnvironment),
            "instrument_failure" if position == 0 => Some(FieldMutation::InstrumentFailure),
            "foreign_session" if position == 1 => Some(FieldMutation::ForeignSession),
            "out_of_order" if position == 1 => Some(FieldMutation::OutOfOrder),
            _ => None,
        };
        mutation.map(|mutation| (mutation, foreign_session.clone()))
    })?;
    if mode == "duplicate_row" && !members.is_empty() {
        // Re-emit the first row's exact bytes: the decoder must retain the
        // first contributor and type the duplicate, never last-writer-wins.
        let first_end = channel
            .rows
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(channel.rows.len());
        let first = channel.rows[..first_end].to_vec();
        channel.rows.extend_from_slice(&first);
        channel.row_count += 1;
    }
    channel.write_trace_with_terminal(ProcessCompletion::ExitStatus { code: 0 })?;
    Ok(0)
}

enum FieldMutation {
    MissingEnvironment,
    InstrumentFailure,
    ForeignSession,
    OutOfOrder,
}

fn emit_all<F>(channel: &mut Channel, members: &[String], mutate: F) -> io::Result<()>
where
    F: Fn(usize) -> Option<(FieldMutation, String)>,
{
    for (position, member) in members.iter().enumerate() {
        let decision = classify(member);
        let mut fields = observed_fields(&decision);
        let mut sequence = position as u32;
        let mut session = channel.session.clone();
        if let Some((mutation, foreign)) = mutate(position) {
            match mutation {
                FieldMutation::MissingEnvironment => {
                    fields.environment = EffectiveInvocationField::NotObserved {
                        reason: "environment not captured at the invocation decision".to_string(),
                    };
                }
                FieldMutation::InstrumentFailure => {
                    fields.scheduling = EffectiveInvocationField::InstrumentFailure {
                        reason: "scheduling capture buffer failed".to_string(),
                    };
                }
                FieldMutation::ForeignSession => {
                    session = foreign;
                }
                FieldMutation::OutOfOrder => {
                    sequence = 5;
                }
            }
        }
        let saved_session = channel.session.clone();
        channel.session = session;
        let emitted = channel.emit_row(&decision, sequence, fields);
        channel.session = saved_session;
        emitted?;
    }
    Ok(())
}

fn session_foreign_to(session: &str) -> String {
    format!("{session}-foreign")
}

fn write_rows(rows: &[String]) -> io::Result<()> {
    let mut stdout = io::stdout();
    for row in rows {
        writeln!(stdout, "{row}")?;
    }
    stdout.flush()
}

/// Collect `.t` rows for the selector roots exactly like the emulated upstream
/// walk: recursive per root, printed as repository-root-relative spellings and
/// sorted, mirroring `_find_tests` plus `dump_tests` at the pinned ref.
fn selected_rows(selectors: &[String]) -> io::Result<Vec<String>> {
    let mut rows = Vec::new();
    for selector in selectors {
        collect_dot_t(Path::new(selector), selector, &mut rows)?;
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn collect_dot_t(dir: &Path, root: &str, rows: &mut Vec<String>) -> io::Result<()> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_dot_t(&path, root, rows)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("t") {
            let relative = path
                .strip_prefix(root)
                .map(|rest| rest.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
            rows.push(format!("t/{root}/{relative}"));
        }
    }
    Ok(())
}

fn park_forever() {
    #[expect(
        clippy::duration_suboptimal_units,
        reason = "the stable Duration constructors stop at seconds; the 60-second park is intentional"
    )]
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
