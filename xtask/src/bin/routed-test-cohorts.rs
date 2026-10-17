//! Staged disk-efficient routed test producer. The policy gate is not activated.
//!
//! Cargo 1.95 resolves the complete selected package/dev-feature union before
//! target filters. Each cohort therefore retains every original -p and feature
//! switch. An admitted check --tests --profile test observes eligible units
//! without bulk executable codegen; metadata is inventory, not feature authority.
//! https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/ops/cargo_compile/mod.rs#L271
//! https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/build_config.rs#L245
#![allow(clippy::print_stdout, clippy::print_stderr)]
use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Instant,
};

#[derive(Debug, Parser)]
#[command(name = "routed-test-cohorts")]
struct Args {
    /// Preserve every original package; patterns/workspace shortcuts are refused.
    #[arg(short = 'p', long = "package", required = true)]
    packages: Vec<String>,
    #[arg(long)]
    features: Vec<String>,
    #[arg(long)]
    all_features: bool,
    #[arg(long)]
    no_default_features: bool,
    /// Exclusive new evidence directory within the canonical worktree.
    #[arg(long)]
    evidence_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct CohortPlan {
    selector: Vec<String>,
    targets: Vec<Value>,
}

fn artifact_key(value: &Value) -> Result<String> {
    let target = value.get("target").ok_or_else(|| eyre!("artifact lacks target"))?;
    Ok(serde_json::to_string(&json!({
        "package_id": value.get("package_id").ok_or_else(|| eyre!("artifact lacks package identity"))?,
        "name": target.get("name"), "kind": target.get("kind"), "src_path": target.get("src_path")
    }))?)
}

fn unsupported_environment_selector(key: &str, value: &std::ffi::OsStr) -> bool {
    key.ends_with("_RUNNER")
        || key == "CARGO_BUILD_TARGET"
        || (!value.is_empty()
            && (key.starts_with("CARGO_TARGET_") && key.ends_with("_RUSTFLAGS")
                || matches!(
                    key,
                    "LD_PRELOAD"
                        | "DYLD_INSERT_LIBRARIES"
                        | "RUSTFLAGS"
                        | "CARGO_ENCODED_RUSTFLAGS"
                        | "CARGO_BUILD_RUSTFLAGS"
                        | "RUSTC_WRAPPER"
                        | "RUSTC_WORKSPACE_WRAPPER"
                        | "CARGO_BUILD_RUSTC_WRAPPER"
                        | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
                )))
}

fn validate_evidence_directory(root: &Path, evidence: &Path) -> Result<()> {
    let canonical_parent = evidence.parent().and_then(|parent| parent.canonicalize().ok());
    let parent_ok = canonical_parent.is_some_and(|parent| {
        evidence.file_name().is_some_and(|name| parent.join(name) == evidence)
            && parent.starts_with(root)
    });
    if !evidence.is_absolute() || !parent_ok || fs::symlink_metadata(evidence).is_ok() {
        bail!("evidence directory must be an exclusive new absolute worktree path");
    }
    Ok(())
}

/// Metadata inventory + real check artifacts, never metadata.resolve.features.
fn plan_cohorts(
    metadata: &Value,
    packages: &[String],
    checked: &[Value],
) -> Result<Vec<CohortPlan>> {
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or_else(|| eyre!("metadata lacks workspace members"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let package_rows =
        metadata["packages"].as_array().ok_or_else(|| eyre!("metadata lacks packages"))?;
    let mut selected = BTreeMap::new();
    for name in packages {
        if name.is_empty()
            || name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        {
            bail!("package patterns/specifiers are unsupported: {name}");
        }
        let matches: Vec<&Value> = package_rows
            .iter()
            .filter(|row| {
                row["name"] == *name && row["id"].as_str().is_some_and(|id| members.contains(id))
            })
            .collect();
        if matches.len() != 1 || selected.contains_key(name) {
            bail!("package must identify one workspace member exactly once: {name}");
        }
        selected.insert(name.clone(), matches[0]);
    }
    let ids: BTreeSet<String> =
        selected.values().filter_map(|row| row["id"].as_str().map(str::to_owned)).collect();
    let mut units = BTreeMap::new();
    for artifact in checked {
        if artifact["reason"] != "compiler-artifact"
            || artifact["profile"]["test"] != true
            || !artifact["package_id"].as_str().is_some_and(|id| ids.contains(id))
        {
            continue;
        }
        if artifact["target"]["test"] != true
            || !artifact["features"].is_array()
            || !artifact["profile"].is_object()
        {
            bail!("unsupported checked test unit");
        }
        let key = artifact_key(artifact)?;
        let exact = json!({"package_id": artifact["package_id"], "target": artifact["target"], "features": artifact["features"], "profile": artifact["profile"]});
        if let Some(previous) = units.insert(key, exact.clone())
            && previous != exact
        {
            bail!("same checked target has conflicting features/profile");
        }
    }
    if units.is_empty() {
        bail!("check did not observe any selected test harness");
    }
    let mut inventory = BTreeMap::new();
    for package in selected.values() {
        for target in
            package["targets"].as_array().ok_or_else(|| eyre!("metadata target list missing"))?
        {
            let identity = json!({"package_id": package["id"], "target": target});
            inventory.insert(artifact_key(&identity)?, (package["id"].clone(), target.clone()));
        }
    }
    for unit in units.values() {
        let key = artifact_key(unit)?;
        let (_, target) =
            inventory.get(&key).ok_or_else(|| eyre!("checked test not in metadata inventory"))?;
        if target["test"] != true {
            bail!("check unexpectedly selected test=false target");
        }
        // Cargo checks are the authority for activated features; required features
        // must be witnessed in that same selected test unit, not workspace resolve.
        if let Some(required) = target["required-features"].as_array() {
            let active =
                unit["features"].as_array().ok_or_else(|| eyre!("missing observed features"))?;
            if required.iter().any(|feature| !active.contains(feature)) {
                bail!("required feature not witnessed by check");
            }
        }
    }
    // Every unconditionally eligible target must be observed; missing required-
    // feature targets may be disabled, but named selector collisions below refuse.
    for (id, target) in inventory.values() {
        if target["test"] == true
            && target["required-features"].as_array().is_none_or(Vec::is_empty)
        {
            let key = artifact_key(&json!({"package_id": id,"target":target}))?;
            if !units.contains_key(&key) {
                bail!("check omitted unconditionally eligible target {key}");
            }
        }
    }
    let mut grouped: BTreeMap<Vec<String>, Vec<Value>> = BTreeMap::new();
    for unit in units.values() {
        let kind = unit["target"]["kind"].as_array().ok_or_else(|| eyre!("target kind missing"))?;
        if kind.len() != 1 {
            bail!("multi-kind test target unsupported");
        }
        let name = unit["target"]["name"].as_str().ok_or_else(|| eyre!("target name missing"))?;
        let selector = match kind[0].as_str() {
            Some("lib" | "proc-macro" | "rlib" | "dylib" | "staticlib" | "cdylib") => {
                vec!["--lib".into()]
            }
            Some("test") => vec!["--test".into(), name.into()],
            Some("bin") => vec!["--bin".into(), name.into()],
            Some("example") => vec!["--example".into(), name.into()],
            Some("bench") => vec!["--bench".into(), name.into()],
            _ => bail!("unsupported --tests target kind"),
        };
        // Named filters select all matching names across ALL original packages,
        // even target.test=false or missing-required-feature targets. Refuse any
        // collision outside original eligible units instead of adding/dropping it.
        for (id, target) in inventory.values() {
            let kinds = target["kind"].as_array().ok_or_else(|| eyre!("metadata kind missing"))?;
            let matches = if selector[0] == "--lib" {
                kinds.iter().any(|kind| {
                    matches!(
                        kind.as_str(),
                        Some("lib" | "proc-macro" | "rlib" | "dylib" | "staticlib" | "cdylib")
                    )
                })
            } else {
                kinds == kind && target["name"] == name
            };
            if matches
                && !units.contains_key(&artifact_key(&json!({"package_id":id,"target":target}))?)
            {
                bail!("global selector collides with ineligible target");
            }
        }
        grouped.entry(selector).or_default().push(unit.clone());
    }
    Ok(grouped.into_iter().map(|(selector, targets)| CohortPlan { selector, targets }).collect())
}

// Bound simultaneously linked harness count; bytes remain subject to original
// capacity admission and measurement. A shared global selector is indivisible.
const MAX_COHORT_HARNESSES: usize = 8;

fn bounded_cohorts(groups: Vec<CohortPlan>) -> Result<Vec<CohortPlan>> {
    if groups.is_empty() {
        bail!("bounded harness cohort plan is empty");
    }
    let mut batches = Vec::new();
    let mut batch = CohortPlan { selector: Vec::new(), targets: Vec::new() };
    for group in groups {
        if group.targets.is_empty() || group.targets.len() > MAX_COHORT_HARNESSES {
            bail!("global selector group exceeds bounded harness cohort capacity");
        }
        if batch.targets.len() + group.targets.len() > MAX_COHORT_HARNESSES {
            batches.push(batch);
            batch = CohortPlan { selector: Vec::new(), targets: Vec::new() };
        }
        batch.selector.extend(group.selector);
        batch.targets.extend(group.targets);
    }
    if !batch.targets.is_empty() {
        batches.push(batch);
    }
    Ok(batches)
}

fn binary_runtime_inventory(package: &Value, package_features: &[Value]) -> Result<Vec<Value>> {
    let mut binaries = Vec::new();
    let mut names = BTreeSet::new();
    for target in package["targets"]
        .as_array()
        .ok_or_else(|| eyre!("package targets missing"))?
        .iter()
        .filter(|target| {
            target["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
    {
        let name = target["name"].as_str().ok_or_else(|| eyre!("binary target name missing"))?;
        if name.is_empty() || name.contains('/') || name.contains('\\') || !names.insert(name) {
            bail!("package metadata has an invalid or repeated binary target name");
        }
        // Cargo omits this optional field when the target has no requirement.
        let enabled = match target.get("required-features") {
            None => true,
            Some(required) => {
                let required = required
                    .as_array()
                    .ok_or_else(|| eyre!("invalid binary required-features inventory"))?;
                if required.iter().any(|feature| !feature.is_string()) {
                    bail!("invalid binary required-features feature name");
                }
                required.iter().all(|feature| package_features.contains(feature))
            }
        };
        binaries.push(json!({"name":name,"enabled":enabled}));
    }
    Ok(binaries)
}

fn target_binary_runtime_inventory(
    package: &Value,
    package_features: &[Value],
    target: &Value,
) -> Result<Option<Vec<Value>>> {
    if matches!(target["kind"][0].as_str(), Some("test" | "bench")) {
        Ok(Some(binary_runtime_inventory(package, package_features)?))
    } else {
        Ok(None)
    }
}

fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let mut encoded = String::with_capacity(64);
    for byte in digest.finalize() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    Ok(encoded)
}
fn write_new(path: &Path, value: &Value) -> Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn original_flags(args: &Args) -> Vec<String> {
    let mut flags = vec!["--locked".into()];
    for package in &args.packages {
        flags.extend(["-p".into(), package.clone()]);
    }
    for features in &args.features {
        flags.extend(["--features".into(), features.clone()]);
    }
    if args.all_features {
        flags.push("--all-features".into());
    }
    if args.no_default_features {
        flags.push("--no-default-features".into());
    }
    flags
}
fn cargo_command(operation: &str, arguments: &[String], descriptor: &Value) -> Result<Vec<String>> {
    let cargo = match env::var("CARGO") {
        Ok(path) => path,
        Err(_) => "cargo".into(),
    };
    Ok(vec![
        cargo,
        "--config".into(),
        "unstable.unstable-options=false".into(),
        "--config".into(),
        format!("build.build-dir={}", serde_json::to_string(&descriptor["resources"]["build"])?),
        "--config".into(),
        "build.rustc-wrapper=\"\"".into(),
        "--config".into(),
        "build.rustc-workspace-wrapper=\"\"".into(),
        operation.into(),
    ]
    .into_iter()
    .chain(arguments.iter().cloned())
    .collect())
}

#[derive(Debug)]
struct BridgeFailure {
    status: u8,
    evidence: PathBuf,
    detail: String,
}
impl std::fmt::Display for BridgeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "cohort status {}; retained evidence {}: {}",
            self.status,
            self.evidence.display(),
            self.detail
        )
    }
}
impl std::error::Error for BridgeFailure {}

fn bridge_failure_status(row: Option<&Value>, child: Option<i32>) -> u8 {
    if let Some(row) = row {
        if row["settlement"]["tree_settled"] == false || row["status"] == 75 {
            return 75;
        }
        if row["settlement"]["cancelled"] == true || row["status"] == 130 {
            return 130;
        }
        if let Some(status) = row["status"]
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|status| *status != 0)
        {
            return status;
        }
    }
    child
        .and_then(|value| u8::try_from(value).ok())
        .filter(|status| *status != 0)
        .map_or(1, |status| status)
}

fn read_bridge_result(path: &Path, child: Option<i32>) -> Result<Value> {
    let decoded: Result<Value> = (|| Ok(serde_json::from_slice(&fs::read(path)?)?))();
    decoded.map_err(|error| {
        BridgeFailure {
            status: bridge_failure_status(None, child),
            evidence: path.to_path_buf(),
            detail: format!("claimed bridge result is unreadable or malformed: {error}"),
        }
        .into()
    })
}

struct Driver {
    root: PathBuf,
    evidence: PathBuf,
    source: PathBuf,
    profile: PathBuf,
    sequence: usize,
    pinned_subjects: BTreeMap<String, Value>,
}
impl Driver {
    fn bridge(
        &mut self,
        operation: &str,
        producer: Vec<String>,
        targets: Vec<Value>,
        consumers: Value,
        pinned: &[String],
    ) -> Result<Value> {
        let id = self.sequence;
        self.sequence += 1;
        let request = self.evidence.join(format!("{id:04}-request.json"));
        let result = self.evidence.join(format!("{id:04}-result.json"));
        write_new(
            &request,
            &json!({"operation":operation,"retain_preexisting":true,"producer":producer,"expected_targets":targets,"consumers":consumers,"pinned":pinned,"pinned_subjects":self.pinned_subjects,
            "source_subject":self.source,"profile_subject":self.profile,"source_digest":digest(&self.source)?,"profile_digest":digest(&self.profile)?,
            "evidence_path":self.evidence.join(format!("{id:04}-compiler.jsonl"))}),
        )?;
        let status = Command::new("python3")
            .arg(self.root.join("scripts/cargo_cohort.py"))
            .arg("--request")
            .arg(&request)
            .arg("--result")
            .arg(&result)
            .current_dir(&self.root)
            .status()?;
        if !result.exists() {
            return Err(BridgeFailure {
                status: bridge_failure_status(None, status.code()),
                evidence: request,
                detail: format!("bridge failed without result: {status}"),
            }
            .into());
        }
        let row = read_bridge_result(&result, status.code())?;
        if !status.success() || row["status"] != 0 || row["settlement"]["tree_settled"] != true {
            return Err(BridgeFailure {
                status: bridge_failure_status(Some(&row), status.code()),
                evidence: result,
                detail: row.to_string(),
            }
            .into());
        }
        Ok(row)
    }
    fn observe(&mut self, command: Vec<String>, pinned: &[String]) -> Result<(Value, Vec<Value>)> {
        let row = self.bridge("observe", command, vec![], json!({}), pinned)?;
        let text = fs::read_to_string(
            row["evidence"].as_str().ok_or_else(|| eyre!("missing observation path"))?,
        )?;
        let messages = text
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<Vec<Value>, _>>()?;
        Ok((row, messages))
    }
}

fn source_manifest(root: &Path, evidence: &Path) -> Result<Value> {
    let output = Command::new("git")
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        bail!("source inventory failed");
    }
    let mut entries = BTreeMap::new();
    for bytes in output.stdout.split(|byte| *byte == 0).filter(|bytes| !bytes.is_empty()) {
        let relative = std::str::from_utf8(bytes).context("non-UTF8 source path unsupported")?;
        let path = root.join(relative);
        if path.starts_with(evidence) || !path.is_file() {
            continue;
        }
        if path.canonicalize()? != path {
            bail!("linked source path unsupported");
        }
        entries.insert(path.clone(), json!({"path":path,"sha256":digest(&path)?}));
    }
    if entries.is_empty() {
        bail!("empty source inventory");
    }
    Ok(json!({"files":entries.into_values().collect::<Vec<_>>() }))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let status = error.downcast_ref::<BridgeFailure>().map_or(1, |failure| failure.status);
            eprintln!("routed-cohort failure: {error}");
            eprintln!("{error:?}");
            ExitCode::from(status)
        }
    }
}

fn run() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    if cfg!(not(target_os = "linux")) {
        bail!("native cohort ownership requires Linux");
    }
    let root = env::current_dir()?.canonicalize()?;
    let descriptor: Value =
        serde_json::from_str(&env::var("CARGO_ADMITTED_COHORT_DESCRIPTOR").context(
            "invoke through cargo-admitted --routed-test-cohorts; no raw Cargo fallback",
        )?)?;
    if descriptor["worktree"] != root.to_string_lossy().as_ref() {
        bail!("outer descriptor worktree mismatch");
    }
    let capability = Command::new("python3")
        .arg(root.join("scripts/cargo_cohort.py"))
        .arg("--capability-only")
        .status()?;
    if !capability.success() {
        bail!("native cancellation capability missing before setup/build");
    }
    for (key, value) in env::vars_os() {
        let key = key.to_str().ok_or_else(|| eyre!("non-UTF8 environment key unsupported"))?;
        if unsupported_environment_selector(key, &value) {
            bail!("unsupported runner/compiler/loader selector: {key}");
        }
    }
    let cargo_home = PathBuf::from(
        descriptor["resources"]["cargo_home"]
            .as_str()
            .ok_or_else(|| eyre!("missing Cargo home"))?,
    );
    if ["config", "config.toml"].iter().any(|name| cargo_home.join(name).exists()) {
        bail!(
            "external Cargo home configuration needs separate effective environment qualification"
        );
    }
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".cargo/config.toml"))?)?;
    if config
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(|targets| targets.values().any(|table| table.get("runner").is_some()))
        || config.get("include").is_some()
        || config.get("build").and_then(|build| build.get("target")).is_some()
    {
        bail!("target runner/config includes require separate qualification");
    }
    if config
        .get("env")
        .and_then(toml::Value::as_table)
        .is_some_and(|entries| entries.keys().any(|key| key != "RUST_BACKTRACE"))
    {
        bail!("custom Cargo environment needs separate effective runtime qualification");
    }
    for parent in root.ancestors().skip(1) {
        if ["config", "config.toml"].iter().any(|name| parent.join(".cargo").join(name).exists()) {
            bail!("parent Cargo configuration needs separate qualification");
        }
    }
    validate_evidence_directory(&root, &args.evidence_dir)?;
    fs::create_dir(&args.evidence_dir)?;
    let intent = args.evidence_dir.join("intent.json");
    write_new(
        &intent,
        &json!({"schema":"routed-cohort-intent.v1","original_argv":original_flags(&args),"baseline":"cargo test --locked --tests","server":"cargo build -p perllsp --locked","activation":"staged only"}),
    )?;
    let source = args.evidence_dir.join("sources.json");
    let profile = args.evidence_dir.join("profile.json");
    write_new(&source, &source_manifest(&root, &args.evidence_dir)?)?;
    write_new(
        &profile,
        &json!({"files":[{"path":intent,"sha256":digest(&intent)?},{"path":root.join("Cargo.toml"),"sha256":digest(&root.join("Cargo.toml"))?},{"path":root.join(".cargo/config.toml"),"sha256":digest(&root.join(".cargo/config.toml"))?}]}),
    )?;
    let mut driver = Driver {
        root: root.clone(),
        evidence: args.evidence_dir.clone(),
        source,
        profile,
        sequence: 0,
        pinned_subjects: BTreeMap::new(),
    };
    let started = Instant::now();
    // Real metadata is observed under the same outer lease; no feature inference.
    let (_, metadata_rows) = driver.observe(
        cargo_command(
            "metadata",
            &["--locked".into(), "--no-deps".into(), "--format-version".into(), "1".into()],
            &descriptor,
        )?,
        &[],
    )?;
    if metadata_rows.len() != 1 {
        bail!("metadata must emit one JSON object");
    }
    let metadata = &metadata_rows[0];
    // Preserve the legacy public server build intent (default features/profile).
    let (_, public) = driver.observe(
        cargo_command(
            "build",
            &[
                "--locked".into(),
                "-p".into(),
                "perllsp".into(),
                "--message-format=json-render-diagnostics".into(),
            ],
            &descriptor,
        )?,
        &[],
    )?;
    let binaries: Vec<&str> = public
        .iter()
        .filter(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == "perllsp"
                && message["profile"]["test"] == false
        })
        .filter_map(|message| message["executable"].as_str())
        .collect();
    if binaries.len() != 1 {
        bail!("public server compiler JSON did not identify exactly one executable");
    }
    let original_public = PathBuf::from(binaries[0]);
    if !["target", "build"].iter().any(|key| {
        descriptor["resources"][key].as_str().is_some_and(|root| original_public.starts_with(root))
    }) || original_public.canonicalize()? != original_public
        || fs::symlink_metadata(&original_public)?.file_type().is_symlink()
    {
        bail!("compiler public executable is not in canonical private resources");
    }
    let public_hash = digest(&original_public)?;
    let immutable_public = args.evidence_dir.join("perllsp-owned-proof");
    // Preserve the compiler-observed source and copy under the same outer lease.
    // No original product is deleted; all cohorts pin the exact immutable copy.
    let copy_program = r#"import hashlib,json,os,sys
src,dst=sys.argv[1:]
facts=lambda s:[s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns,s.st_mode,s.st_nlink]
source=os.open(src,os.O_RDONLY|os.O_NOFOLLOW)
try:
 before=os.fstat(source)
 dest=os.open(dst,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,before.st_mode & 0o777)
 try:
  digest=hashlib.sha256()
  while chunk:=os.read(source,65536):
   digest.update(chunk);remaining=memoryview(chunk)
   while remaining:
    written=os.write(dest,remaining)
    if written<=0:raise OSError('public copy made no write progress')
    remaining=remaining[written:]
  os.fchmod(dest,before.st_mode & 0o777);os.fsync(dest)
  if facts(before)!=facts(os.fstat(source)) or facts(before)!=facts(os.stat(src,follow_symlinks=False)):raise OSError('source changed during public copy')
  directory=os.open(os.path.dirname(dst),os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
  try:os.fsync(directory)
  finally:os.close(directory)
  print(json.dumps({'source_identity':facts(before),'copy_identity':facts(os.fstat(dest)),'sha256':digest.hexdigest()}))
 finally:os.close(dest)
finally:os.close(source)
"#;
    let copy_command = vec![
        "python3".into(),
        "-c".into(),
        copy_program.into(),
        original_public.to_string_lossy().into_owned(),
        immutable_public.to_string_lossy().into_owned(),
    ];
    let (_, copy_rows) = driver.observe(copy_command, &[])?;
    if copy_rows.len() != 1
        || copy_rows[0]["sha256"] != public_hash
        || digest(&original_public)? != public_hash
        || digest(&immutable_public)? != public_hash
    {
        bail!("public executable changed during immutable copy");
    }
    let pinned = vec![
        immutable_public.to_string_lossy().into_owned(),
        original_public.to_string_lossy().into_owned(),
    ];
    driver.pinned_subjects.insert(
        pinned[0].clone(),
        json!({"sha256":public_hash,"file_identity":copy_rows[0]["copy_identity"]}),
    );
    write_new(
        &args.evidence_dir.join("public-subject.json"),
        &json!({"compiler_executable":original_public,"immutable_executable":immutable_public,"source_before_after_identity":copy_rows[0]["source_identity"],"copy_identity":copy_rows[0]["copy_identity"],"sha256":public_hash}),
    )?;
    let mut flags = original_flags(&args);
    flags.extend([
        "--tests".into(),
        "--profile".into(),
        "test".into(),
        "--message-format=json-render-diagnostics".into(),
    ]);
    let (_, checked) = driver.observe(cargo_command("check", &flags, &descriptor)?, &pinned)?;
    let rustc = match env::var("RUSTC") {
        Ok(path) => path,
        Err(_) => "rustc".into(),
    };
    let libdir_row = driver.bridge(
        "observe",
        vec![rustc, "--print".into(), "target-libdir".into()],
        vec![],
        json!({}),
        &pinned,
    )?;
    let sysroot_lib = fs::read_to_string(
        libdir_row["evidence"]
            .as_str()
            .ok_or_else(|| eyre!("missing compiler runtime evidence"))?,
    )?
    .trim()
    .to_owned();
    if !Path::new(&sysroot_lib).is_absolute() || !Path::new(&sysroot_lib).is_dir() {
        bail!("compiler runtime directory unavailable");
    }
    let target_dir = PathBuf::from(
        descriptor["resources"]["target"]
            .as_str()
            .ok_or_else(|| eyre!("missing target resource"))?,
    );
    let debug_root = target_dir.join("debug");
    let mut native_dirs = BTreeSet::new();
    for message in &checked {
        if let Some(paths) = message["linked_paths"].as_array() {
            for path in paths {
                if let Some(path) = path.as_str() {
                    let path = PathBuf::from(path.split_once('=').map_or(path, |(_, value)| value));
                    // Cargo filter_dynamic_search_path excludes native paths
                    // outside this root output; do not broaden loader lookup.
                    if path.starts_with(&debug_root) {
                        native_dirs.insert(path);
                    }
                }
            }
        }
    }
    let mut dylib_dirs: Vec<PathBuf> = native_dirs.into_iter().collect();
    dylib_dirs.extend([debug_root.clone(), debug_root.join("deps"), PathBuf::from(sysroot_lib)]);
    if let Some(existing) = descriptor["runtime_environment"]["LD_LIBRARY_PATH"].as_str() {
        let existing: Vec<PathBuf> = env::split_paths(existing).collect();
        if existing.starts_with(&dylib_dirs) {
            dylib_dirs = existing;
        } else {
            dylib_dirs.extend(existing);
        }
    }
    let loader = env::join_paths(dylib_dirs)?.to_string_lossy().into_owned();
    let cohorts = bounded_cohorts(plan_cohorts(metadata, &args.packages, &checked)?)?;
    let mut receipts = vec![];
    for cohort in cohorts {
        let mut arguments = original_flags(&args);
        arguments.extend(cohort.selector.clone());
        arguments.extend(["--no-run".into(), "--message-format=json-render-diagnostics".into()]);
        // Executable paths are not guessable from target names. Consumers are
        // keyed by target identity and the bridge maps only captured JSON paths.
        let mut consumers = serde_json::Map::new();
        for target in &cohort.targets {
            let package = metadata["packages"]
                .as_array()
                .and_then(|rows| rows.iter().find(|package| package["id"] == target["package_id"]))
                .ok_or_else(|| eyre!("target package missing"))?;
            let cwd = PathBuf::from(
                package["manifest_path"].as_str().ok_or_else(|| eyre!("manifest path missing"))?,
            )
            .parent()
            .ok_or_else(|| eyre!("manifest parent missing"))?
            .to_path_buf();
            let mut runtime = serde_json::Map::new();
            let package_features = target["features"]
                .as_array()
                .ok_or_else(|| eyre!("test unit feature set missing"))?;
            let cargo_bin_exe =
                target_binary_runtime_inventory(package, package_features, &target["target"])?;
            // Cargo exposes package build-script extra_env to its test process.
            for message in &checked {
                if message["reason"] == "build-script-executed"
                    && message["package_id"] == target["package_id"]
                    && let Some(pairs) = message["env"].as_array()
                {
                    for pair in pairs {
                        let pair =
                            pair.as_array().ok_or_else(|| eyre!("invalid build-script env"))?;
                        if pair.len() != 2 {
                            bail!("invalid build-script env pair");
                        }
                        runtime.insert(
                            pair[0]
                                .as_str()
                                .ok_or_else(|| eyre!("invalid build-script env key"))?
                                .into(),
                            pair[1].clone(),
                        );
                    }
                }
            }
            runtime.insert(
                "CARGO".into(),
                json!(match env::var("CARGO") {
                    Ok(path) => path,
                    Err(_) => "cargo".into(),
                }),
            );
            runtime.insert("LD_LIBRARY_PATH".into(), json!(loader));
            runtime.insert("PERL_LSP_BIN".into(), json!(pinned[0]));
            runtime.insert("CARGO_MANIFEST_DIR".into(), json!(cwd));
            runtime.insert("CARGO_MANIFEST_PATH".into(), package["manifest_path"].clone());
            for (field, key) in [
                ("name", "CARGO_PKG_NAME"),
                ("version", "CARGO_PKG_VERSION"),
                ("description", "CARGO_PKG_DESCRIPTION"),
                ("license", "CARGO_PKG_LICENSE"),
                ("repository", "CARGO_PKG_REPOSITORY"),
                ("homepage", "CARGO_PKG_HOMEPAGE"),
                ("license_file", "CARGO_PKG_LICENSE_FILE"),
                ("readme", "CARGO_PKG_README"),
                ("rust_version", "CARGO_PKG_RUST_VERSION"),
            ] {
                runtime
                    .insert(key.into(), json!(package[field].as_str().map_or("", |value| value)));
            }
            if let Some(authors) = package["authors"].as_array() {
                runtime.insert(
                    "CARGO_PKG_AUTHORS".into(),
                    json!(authors.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(":")),
                );
            }
            let version =
                package["version"].as_str().ok_or_else(|| eyre!("package version unavailable"))?;
            let parts: Vec<&str> = version.split(['.', '-', '+']).collect();
            if parts.len() < 3 {
                bail!("unsupported package version");
            }
            for (key, value) in [
                ("CARGO_PKG_VERSION_MAJOR", parts[0]),
                ("CARGO_PKG_VERSION_MINOR", parts[1]),
                ("CARGO_PKG_VERSION_PATCH", parts[2]),
                (
                    "CARGO_PKG_VERSION_PRE",
                    version
                        .split_once('-')
                        .map_or("", |(_, pre)| pre.split('+').next().map_or("", |value| value)),
                ),
            ] {
                runtime.insert(key.into(), json!(value));
            }
            let scripts:Vec<Value> = checked.iter().filter(|message|message["reason"]=="build-script-executed" && message["package_id"]==target["package_id"]).map(|message|json!({"package_id":message["package_id"],"env":message["env"],"linked_paths":message["linked_paths"],"out_dir":message["out_dir"]})).collect();
            if scripts.len() > 1 {
                bail!("ambiguous package build-script unit contexts");
            }
            let mut consumer = json!({"args":[],"cwd":cwd,"env":runtime,"verify_harness":true,"build_scripts":scripts});
            if let Some(inventory) = cargo_bin_exe {
                consumer["cargo_bin_exe"] = json!(inventory);
            }
            consumers.insert(artifact_key(target)?, consumer);
        }
        let row = driver.bridge(
            "run",
            cargo_command("test", &arguments, &descriptor)?,
            cohort.targets,
            json!({"by_target":consumers}),
            &pinned,
        )?;
        receipts.push(row);
    }
    write_new(
        &args.evidence_dir.join("coverage.json"),
        &json!({"schema":"routed-test-coverage.v1","packages":args.packages,"argv":original_flags(&args),"elapsed_ms":started.elapsed().as_millis(),"cohorts":receipts,"activation":"NOT_PROVEN until native exact-candidate qualification"}),
    )?;
    println!("routed cohort evidence: {}", args.evidence_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_binary_required_features_means_unconditionally_enabled() -> Result<()> {
        let package = json!({"targets":[{"name":"ordinary","kind":["bin"]}]});
        assert_eq!(
            binary_runtime_inventory(&package, &[])?,
            vec![json!({"name":"ordinary","enabled":true})]
        );
        for invalid in [Value::Null, json!("feature"), json!([false])] {
            let malformed =
                json!({"targets":[{"name":"ordinary","kind":["bin"],"required-features":invalid}]});
            assert!(binary_runtime_inventory(&malformed, &[]).is_err());
        }
        Ok(())
    }

    #[test]
    fn binary_runtime_environment_is_only_for_integration_and_bench_units() -> Result<()> {
        let package = json!({"targets":[{"name":"ordinary","kind":["bin"]}]});
        for kind in ["lib", "bin", "example"] {
            assert!(
                target_binary_runtime_inventory(&package, &[], &json!({"kind":[kind]}))?.is_none()
            );
        }
        for kind in ["test", "bench"] {
            assert_eq!(
                target_binary_runtime_inventory(&package, &[], &json!({"kind":[kind]}))?,
                Some(vec![json!({"name":"ordinary","enabled":true})])
            );
        }
        Ok(())
    }

    #[test]
    fn compiler_selectors_refuse_cargo_config_environment_aliases() {
        for key in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        ] {
            assert!(unsupported_environment_selector(key, std::ffi::OsStr::new("value")));
            assert!(!unsupported_environment_selector(key, std::ffi::OsStr::new("")));
        }
        for key in ["CARGO_BUILD_TARGET", "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER"] {
            assert!(unsupported_environment_selector(key, std::ffi::OsStr::new("")));
        }
        assert!(!unsupported_environment_selector(
            "CARGO_TERM_COLOR",
            std::ffi::OsStr::new("always")
        ));
    }

    #[test]
    fn evidence_parent_must_be_canonical_and_inside_worktree() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        assert!(validate_evidence_directory(&root, &root.join("new")).is_ok());
        assert!(validate_evidence_directory(&root, &root.join("../outside")).is_err());
        assert!(validate_evidence_directory(&root, &root).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root, root.join("parent-alias"))?;
            assert!(validate_evidence_directory(&root, &root.join("parent-alias/new")).is_err());
            std::os::unix::fs::symlink(root.join("missing"), root.join("dangling"))?;
            assert!(validate_evidence_directory(&root, &root.join("dangling")).is_err());
        }
        Ok(())
    }
    fn fixture() -> (Value, Vec<String>, Vec<Value>) {
        let a = json!({"name":"shared","kind":["test"],"src_path":"/fixture/a/tests/shared.rs","test":true,"required-features":["activated"]});
        let b = json!({"name":"shared","kind":["test"],"src_path":"/fixture/b/tests/shared.rs","test":true,"required-features":[]});
        let metadata = json!({"workspace_members":["a","b"],"packages":[{"name":"a","id":"a","targets":[a]},{"name":"b","id":"b","targets":[b]}]});
        let checked = vec![
            json!({"reason":"compiler-artifact","package_id":"a","target":a,"features":["activated"],"profile":{"test":true}}),
            json!({"reason":"compiler-artifact","package_id":"b","target":b,"features":[],"profile":{"test":true}}),
        ];
        (metadata, vec!["a".into(), "b".into()], checked)
    }
    #[test]
    fn duplicate_names_keep_full_package_and_dev_feature_union() -> Result<()> {
        let (metadata, packages, checked) = fixture();
        let plan = plan_cohorts(&metadata, &packages, &checked)?;
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].targets.len(), 2);
        assert_eq!(plan[0].selector, vec!["--test", "shared"]);
        Ok(())
    }
    #[test]
    fn bounded_batches_preserve_exact_indivisible_selector_union() -> Result<()> {
        assert!(bounded_cohorts(vec![]).is_err());
        let groups: Vec<CohortPlan> = (0..10)
            .map(|index| CohortPlan {
                selector: vec!["--test".into(), format!("case_{index}")],
                targets: vec![
                    json!({"identity":index,"features":["original"],"profile":{"test":true}}),
                ],
            })
            .collect();
        let selectors: Vec<String> =
            groups.iter().flat_map(|group| group.selector.clone()).collect();
        let targets: Vec<Value> = groups.iter().flat_map(|group| group.targets.clone()).collect();
        let batches = bounded_cohorts(groups)?;
        assert_eq!(batches.iter().map(|batch| batch.targets.len()).collect::<Vec<_>>(), vec![8, 2]);
        assert_eq!(
            batches.iter().flat_map(|batch| batch.selector.clone()).collect::<Vec<_>>(),
            selectors
        );
        assert_eq!(
            batches.iter().flat_map(|batch| batch.targets.clone()).collect::<Vec<_>>(),
            targets
        );
        let shared = vec![
            CohortPlan { selector: vec!["--lib".into()], targets: vec![json!("lib"); 6] },
            CohortPlan {
                selector: vec!["--test".into(), "shared".into()],
                targets: vec![json!("test"); 3],
            },
        ];
        let batches = bounded_cohorts(shared)?;
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].targets.len(), 6);
        assert_eq!(batches[1].targets.len(), 3);
        assert!(
            bounded_cohorts(vec![CohortPlan {
                selector: vec!["--lib".into()],
                targets: vec![json!("lib"); 9]
            }])
            .is_err()
        );
        assert!(
            bounded_cohorts(vec![CohortPlan { selector: vec!["--lib".into()], targets: vec![] }])
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn wrong_required_feature_receipt_refused() {
        let (metadata, packages, mut checked) = fixture();
        checked[0]["features"] = json!([]);
        assert!(plan_cohorts(&metadata, &packages, &checked).is_err());
    }
    #[test]
    fn bin_owning_integration_and_benchmark_targets_keep_their_selectors() -> Result<()> {
        let (mut metadata, packages, checked) = fixture();
        let targets = metadata["packages"][1]["targets"]
            .as_array_mut()
            .ok_or_else(|| eyre!("fixture targets must be an array"))?;
        targets.extend([
            json!({"name":"enabled_probe","kind":["bin"],"src_path":"/fixture/b/src/enabled_probe.rs","test":false,"required-features":["enabled-bin"]}),
            json!({"name":"unbuilt_probe","kind":["bin"],"src_path":"/fixture/b/src/unbuilt_probe.rs","test":false,"required-features":["unbuilt"]}),
        ]);
        for kind in ["test", "bench"] {
            let mut metadata = metadata.clone();
            metadata["packages"][1]["targets"]
                .as_array_mut()
                .ok_or_else(|| eyre!("fixture targets must be an array"))?
                .retain(|target| target["name"] != "shared");
            let mut checked = vec![checked[0].clone()];
            let target = json!({"name":"runtime_env","kind":[kind],"src_path":"/fixture/b/tests/runtime_env.rs","test":true,"required-features":[]});
            metadata["packages"][1]["targets"]
                .as_array_mut()
                .ok_or_else(|| eyre!("fixture targets must be an array"))?
                .push(target.clone());
            checked.push(json!({"reason":"compiler-artifact","package_id":"b","target":target,"features":[],"profile":{"test":true}}));
            let plan = plan_cohorts(&metadata, &packages, &checked)?;
            assert!(
                plan.iter().any(|cohort| cohort
                    .targets
                    .iter()
                    .any(|unit| unit["target"]["kind"][0] == kind))
            );
        }
        Ok(())
    }
    #[test]
    fn cargo_bin_runtime_inventory_keeps_enabled_and_disabled_targets() -> Result<()> {
        let package = json!({"targets":[
            {"name":"enabled_probe","kind":["bin"],"required-features":["enabled-bin"]},
            {"name":"unbuilt_probe","kind":["bin"],"required-features":["unbuilt"]},
            {"name":"library","kind":["lib"],"required-features":[]}
        ]});
        let inventory = binary_runtime_inventory(&package, &[json!("enabled-bin")])?;
        assert_eq!(
            inventory,
            vec![
                json!({"name":"enabled_probe","enabled":true}),
                json!({"name":"unbuilt_probe","enabled":false})
            ]
        );
        let duplicate = json!({"targets":[
            {"name":"same","kind":["bin"],"required-features":[]},
            {"name":"same","kind":["bin"],"required-features":[]}
        ]});
        assert!(binary_runtime_inventory(&duplicate, &[]).is_err());
        Ok(())
    }
    #[test]
    fn inner_closure_and_cancellation_status_survive_native_wrapper() {
        let unsettled = json!({"status":75,"product_status":0,"settlement":{"tree_settled":false,"cancelled":false}});
        let cancelled = json!({"status":130,"product_status":0,"settlement":{"tree_settled":true,"cancelled":true}});
        let ordinary = json!({"status":101,"product_status":101,"settlement":{"tree_settled":true,"cancelled":false}});
        assert_eq!(bridge_failure_status(Some(&unsettled), Some(1)), 75);
        assert_eq!(bridge_failure_status(Some(&cancelled), Some(1)), 130);
        assert_eq!(bridge_failure_status(Some(&ordinary), Some(1)), 101);
        assert_eq!(bridge_failure_status(None, Some(75)), 75);
        let error: color_eyre::eyre::Report = BridgeFailure {
            status: 75,
            evidence: PathBuf::from("/retained"),
            detail: "unproven closure".into(),
        }
        .into();
        assert_eq!(error.downcast_ref::<BridgeFailure>().map(|failure| failure.status), Some(75));
    }
    #[test]
    fn malformed_claimed_result_preserves_child_failure_and_evidence() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("claimed.json");
        for bytes in [b"".as_slice(), b"{".as_slice()] {
            fs::write(&path, bytes)?;
            for status in [75, 130, 101] {
                let error = match read_bridge_result(&path, Some(status)) {
                    Err(error) => error,
                    Ok(_) => bail!("malformed bridge result was accepted"),
                };
                let failure = error
                    .downcast_ref::<BridgeFailure>()
                    .ok_or_else(|| eyre!("missing typed bridge failure"))?;
                assert_eq!(failure.status, status as u8);
                assert_eq!(failure.evidence, path);
                assert_eq!(fs::read(&path)?, bytes);
            }
        }
        fs::write(&path, br#"{"status":0}"#)?;
        assert_eq!(read_bridge_result(&path, Some(0))?, json!({"status":0}));
        Ok(())
    }
    #[test]
    fn every_original_package_and_feature_switch_preserved() {
        let args = Args {
            packages: vec!["cohort-a".into(), "cohort-b".into()],
            features: vec!["cohort-a/activated".into()],
            all_features: true,
            no_default_features: true,
            evidence_dir: PathBuf::from("/evidence"),
        };
        assert_eq!(
            original_flags(&args),
            vec![
                "--locked",
                "-p",
                "cohort-a",
                "-p",
                "cohort-b",
                "--features",
                "cohort-a/activated",
                "--all-features",
                "--no-default-features"
            ]
        );
    }
    #[test]
    fn disabled_or_test_false_name_collision_refused() {
        let (mut metadata, packages, checked) = fixture();
        metadata["packages"][1]["targets"][0]["test"] = json!(false);
        assert!(plan_cohorts(&metadata, &packages, &checked).is_err());
        assert!(plan_cohorts(&metadata, &packages, &checked[..1]).is_err());
    }
}
