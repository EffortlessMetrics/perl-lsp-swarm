//! The `perl-ripr-facts` standalone binary's `ripr-facts` subcommand
//! ([`run_cli`]) and the thin `run_ripr_facts`/`run_ripr_facts_with_diff`
//! wrapper the `perl-lsp` / `perllsp` `ripr-facts` subcommand calls: argv
//! parsing, output-path validation, writing the packet to disk, and mapping
//! to a process exit code. All the actual fact production happens in
//! [`crate::packet::build_ripr_facts_packet`].

use crate::packet::build_ripr_facts_packet;
use crate::request::{EXPECTED_RIPR_FACTS_SCHEMA, RiprFactsRequest, validate_ripr_facts_path};

const DEFAULT_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";
const DEFAULT_OUT: &str = "target/ripr/reports/perl-facts.json";

#[derive(Debug, Clone, Eq, PartialEq)]
struct RiprFactsCli {
    schema: String,
    root: String,
    base: Option<String>,
    head: Option<String>,
    fact_classes: String,
    diff_path: Option<String>,
    out: String,
}

impl Default for RiprFactsCli {
    fn default() -> Self {
        Self {
            schema: EXPECTED_RIPR_FACTS_SCHEMA.to_string(),
            root: ".".to_string(),
            base: None,
            head: None,
            fact_classes: DEFAULT_FACT_CLASSES.to_string(),
            diff_path: None,
            out: DEFAULT_OUT.to_string(),
        }
    }
}

#[expect(
    clippy::print_stderr,
    reason = "ripr-facts is a batch CLI unit — user-facing diagnostics intentionally use stderr"
)]
pub fn run_cli<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let cli = match parse_ripr_facts_cli(&args) {
        Ok(cli) => cli,
        Err(reason) => {
            eprintln!("ripr-facts: {reason}");
            eprintln!("{}", ripr_facts_usage());
            return 1;
        }
    };

    let diff_text = match cli.diff_path.as_deref() {
        Some(path) => match read_diff_text(&cli.root, path) {
            Ok(text) => Some(text),
            Err(reason) => {
                eprintln!("ripr-facts: {reason}");
                return 1;
            }
        },
        None => None,
    };

    run_ripr_facts_with_diff(
        &cli.schema,
        &cli.root,
        cli.base.as_deref(),
        cli.head.as_deref(),
        &cli.fact_classes,
        diff_text.as_deref(),
        &cli.out,
    )
}

fn parse_ripr_facts_cli(args: &[String]) -> Result<RiprFactsCli, String> {
    let mut iter = args.iter();
    let _program = iter.next();
    match iter.next().map(String::as_str) {
        Some("ripr-facts") => {}
        Some("--help" | "-h") => return Err("missing subcommand `ripr-facts`".to_string()),
        Some(other) => return Err(format!("unexpected subcommand or option `{other}`")),
        None => return Err("missing subcommand `ripr-facts`".to_string()),
    }

    let rest: Vec<&str> = iter.map(String::as_str).collect();
    let mut cli = RiprFactsCli::default();
    let mut index = 0usize;
    while index < rest.len() {
        let flag = rest[index];
        let value = rest.get(index + 1).ok_or_else(|| format!("missing value for `{flag}`"))?;
        match flag {
            "--schema" => cli.schema = (*value).to_string(),
            "--root" => cli.root = (*value).to_string(),
            "--base" => cli.base = Some((*value).to_string()),
            "--head" => cli.head = Some((*value).to_string()),
            "--fact-classes" => cli.fact_classes = (*value).to_string(),
            "--diff" => cli.diff_path = Some((*value).to_string()),
            "--out" => cli.out = (*value).to_string(),
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 2;
    }

    Ok(cli)
}

fn ripr_facts_usage() -> &'static str {
    "usage: perl-ripr-facts ripr-facts --schema ripr-perl-facts-v1 --root <root> \
     [--base <base>] [--head <head>] [--fact-classes <classes>] \
     [--diff <cwd-relative-diff>] --out <out>"
}

fn read_diff_text(root: &str, diff_path: &str) -> Result<String, String> {
    validate_ripr_facts_path(root, "root")?;
    validate_ripr_facts_path(diff_path, "diff")?;
    let path = std::path::Path::new(diff_path);
    std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read diff `{}`: {error}", path.display()))
}

/// Run the `ripr-facts` exporter (Campaign 31, ripr-swarm#1379).
///
/// The thin CLI wrapper over [`build_ripr_facts_packet`]: it forwards the
/// CLI-shaped args to the batch API, then validates the output path, writes the
/// assembled packet to `out`, and maps the outcome to a process exit code (`0`
/// on success, `1` on any validation or write failure). Diagnostics go to
/// stderr with a `ripr-facts: ` prefix.
///
/// The `out` path (a write concern owned by the wrapper, not part of the
/// packet) is validated first: it is the cheapest check, so failing on it before
/// building the packet avoids a needless workspace scan when the write
/// destination is invalid.
pub fn run_ripr_facts(
    schema: &str,
    root: &str,
    base: Option<&str>,
    head: Option<&str>,
    fact_classes: &str,
    out: &str,
) -> i32 {
    run_ripr_facts_with_diff(schema, root, base, head, fact_classes, None, out)
}

#[expect(
    clippy::print_stderr,
    reason = "ripr-facts is a batch CLI unit — user-facing diagnostics intentionally use stderr"
)]
pub fn run_ripr_facts_with_diff(
    schema: &str,
    root: &str,
    base: Option<&str>,
    head: Option<&str>,
    fact_classes: &str,
    diff: Option<&str>,
    out: &str,
) -> i32 {
    // Validate the output path first — the cheapest check — so an invalid write
    // destination fails fast, before the emitter scans the workspace.
    if let Err(reason) = validate_ripr_facts_path(out, "out") {
        eprintln!("ripr-facts: {reason}");
        return 1;
    }

    let packet = match build_ripr_facts_packet(&RiprFactsRequest {
        schema,
        root,
        base,
        head,
        fact_classes,
        diff,
    }) {
        Ok(packet) => packet,
        Err(error) => {
            eprintln!("ripr-facts: {error}");
            return 1;
        }
    };

    // Write the assembled packet to disk.
    if let Err(error) = write_packet(out, &packet) {
        eprintln!("ripr-facts: failed to write packet to `{out}`: {error}");
        return 1;
    }

    let status = packet["packet_status"].as_str().unwrap_or("unknown");
    eprintln!("ripr-facts: wrote {status} packet to `{out}`");
    0
}

/// Write a JSON packet to the output path, creating parent directories.
///
/// The destination is **replaced**, never rewritten in place (#16022, #8165
/// "Output safety"): the fully serialized packet is staged in a temporary
/// sibling inside the destination directory, flushed and synced, and only then
/// renamed over `out`. `std::fs::rename` replaces the destination in one step,
/// so a RIPR consumer reading `out` observes either the previous complete
/// packet or the new complete packet — never a truncated or half-written one.
///
/// A plain `std::fs::write` cannot offer that: it opens the destination with
/// `O_TRUNC` (`CREATE_ALWAYS` on Windows), destroying the previous valid packet
/// before the first new byte lands. Any mid-write failure — `ENOSPC`, `EIO`, a
/// cancelled CI job, a killed producer — then leaves invalid JSON where a
/// complete fact packet used to be, which downstream consumers cannot tell
/// apart from a genuinely empty analysis.
///
/// The sibling is staged in the destination's own directory, not the system
/// temp directory, so the rename stays within one filesystem; a cross-device
/// rename fails with `EXDEV` and would defeat the recipe.
fn write_packet(out: &str, packet: &serde_json::Value) -> std::io::Result<()> {
    // Replace the file `std::fs::write` would have written through, not the
    // link pointing at it: `fs::write` follows a symlinked destination, while
    // `fs::rename` would replace the link itself and strand its target.
    let path = resolve_destination(std::path::Path::new(out));
    let path = path.as_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Serialize in full before touching the filesystem: a serialization failure
    // must leave the destination untouched, not partially rewritten.
    let json = serde_json::to_string_pretty(packet)?;

    let temp = stage_temp_sibling(path, json.as_bytes())?;
    carry_destination_permissions(path, &temp);
    if let Err(error) = std::fs::rename(&temp, path) {
        // Best-effort cleanup: only the sibling we actually created is ours, and
        // it is scratch. A rename failure is reported as such.
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// Follow a symlinked destination to the file it names.
///
/// `std::fs::write` follows the link and updates its target, keeping the link
/// in place. A bare `fs::rename` onto the link would instead replace the link
/// with a regular file and leave the real target frozen at its last contents,
/// silently breaking any `latest.json -> packet-vN.json` style publication.
/// Resolving first keeps the pre-existing behavior and still replaces the real
/// file atomically, because the staged sibling then lands beside *it*.
///
/// Only the final component is followed, and the walk is bounded so a symlink
/// cycle terminates instead of spinning. A broken or unreadable link resolves
/// to itself, which lets the ordinary create path report the real error.
fn resolve_destination(path: &std::path::Path) -> std::path::PathBuf {
    /// Matches the conventional `SYMLOOP_MAX` floor; deeper chains are cycles
    /// for this purpose.
    const MAX_LINK_DEPTH: usize = 8;

    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINK_DEPTH {
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            return current;
        };
        if !metadata.file_type().is_symlink() {
            return current;
        }
        let Ok(target) = std::fs::read_link(&current) else {
            return current;
        };
        current = match current.parent() {
            // A relative link resolves against the directory holding the link.
            Some(parent) if !parent.as_os_str().is_empty() && target.is_relative() => {
                parent.join(target)
            }
            _ => target,
        };
    }
    current
}

/// Carry an existing destination's permissions onto the staged sibling.
///
/// `std::fs::write` reuses the destination's inode, so its mode survives a
/// rewrite. A staged file is new, so it would otherwise publish with the
/// process umask and silently reset an operator's `chmod 600` on every run.
/// Best-effort by design: when there is no previous destination the umask
/// default is correct, and a platform that cannot read or apply the mode must
/// not fail an otherwise complete packet.
fn carry_destination_permissions(destination: &std::path::Path, staged: &std::path::Path) {
    if let Ok(metadata) = std::fs::metadata(destination) {
        let _ = std::fs::set_permissions(staged, metadata.permissions());
    }
}

/// Per-process sequence making each staged sibling unique, so two writers in
/// one process (parallel `cargo test` threads, a future concurrent producer)
/// aimed at the same destination stage into separate files. Without it they
/// would share one scratch path and interleave bytes into it.
static TEMP_SIBLING_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Build the staging path for `path`: a dot-prefixed, `.tmp`-marked sibling in
/// the same directory, scoped by process id and an in-process sequence so no
/// two concurrent writers collide.
fn temp_sibling_path(path: &std::path::Path) -> std::path::PathBuf {
    let sequence = TEMP_SIBLING_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("perl-facts.json"), std::ffi::OsStr::to_os_string);

    let mut staged = std::ffi::OsString::from(".");
    staged.push(&file_name);
    staged.push(format!(".tmp-{}-{sequence}", std::process::id()));

    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(staged),
        _ => std::path::PathBuf::from(staged),
    }
}

/// Maximum number of attempts to claim an exclusive staging sibling before
/// reporting the collision as exhaustion. Each attempt computes a fresh path
/// via [`temp_sibling_path`], so the bound is on attempts per destination
/// call, not on the sequence counter itself.
const MAX_TEMP_SIBLING_ATTEMPTS: usize = 16;

/// Write `bytes` to an exclusively-created staged sibling and make them
/// durable, so the rename that follows publishes a complete file rather than
/// one whose contents are still only in the page cache. Returns the path of
/// the scratch file actually created, so the caller can clean it up on
/// rename failure and never touches a sibling this invocation did not write.
///
/// Staging uses `O_CREAT | O_EXCL` semantics (`OpenOptions::create_new`) so a
/// stale sibling left over from a previous interrupted run is preserved
/// rather than truncated. `AlreadyExists` is recovered by trying the next
/// sequence number up to [`MAX_TEMP_SIBLING_ATTEMPTS`] — the destination
/// itself is never touched, and a pre-existing scratch file that this
/// invocation did not create is never removed.
fn stage_temp_sibling(dest: &std::path::Path, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let mut last_collision: Option<std::io::Error> = None;
    for _ in 0..MAX_TEMP_SIBLING_ATTEMPTS {
        let temp = temp_sibling_path(dest);
        let file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // The computed path already names a real file. This is the
                // stale-sibling case the issue describes: another run (or
                // another part of this process) created a scratch path with
                // a sequence number we would otherwise have used. We must
                // not truncate it, and we must not delete it: it might not
                // be ours. Try the next sequence number.
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        finish_staged_file(file, &temp, bytes)?;
        return Ok(temp);
    }
    Err(last_collision.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "exhausted {MAX_TEMP_SIBLING_ATTEMPTS} attempts to allocate an exclusive \
                 staging sibling for `{}`",
                dest.display()
            ),
        )
    }))
}

/// Finish the staged write for a sibling this invocation exclusively created.
///
/// On any write, flush, or sync failure the owned scratch is best-effort
/// removed before the original error is returned, so repeated staging
/// failures do not accumulate partial scratch files that a later run must
/// treat as strangers. Only call with a path this invocation created: a
/// pre-existing file passed here is removed on failure.
fn finish_staged_file(
    file: std::fs::File,
    temp: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut file = file;
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    })();
    if let Err(error) = result {
        // Close before removing: deleting an open handle fails on Windows,
        // which would leave the partial scratch behind on that platform.
        drop(file);
        let _ = std::fs::remove_file(temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::build_unavailable_packet;
    use perl_tdd_support::{must_some, must_some_with, must_with};

    /// A valid request against the crate root (`"."`, no `t/` dir → unavailable).
    /// Local copy of `crate::packet`'s test-only helper of the same name — see
    /// #9271 PR notes: duplicated rather than exposed cross-module, since it
    /// is a one-line fixture builder.
    fn valid_request<'a>(fact_classes: &'a str) -> RiprFactsRequest<'a> {
        RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root: ".",
            base: Some("origin/main"),
            head: Some("HEAD"),
            fact_classes,
            diff: None,
        }
    }

    #[test]
    fn build_packet_matches_what_the_wrapper_writes() -> std::io::Result<()> {
        // Parity: the packet the batch API returns is byte-identical to what the
        // `run_ripr_facts` CLI wrapper writes to disk for the same inputs.
        let out = "target/ripr/test-batch-parity.json";
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            ".",
            Some("origin/main"),
            Some("HEAD"),
            "tests,oracles",
            out,
        );
        assert_eq!(rc, 0, "wrapper must succeed");
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out)?)?;
        let built = must_with(
            build_ripr_facts_packet(&valid_request("tests,oracles")),
            "valid request builds a packet",
        );
        assert_eq!(built, written, "batch API packet must equal what the wrapper writes");
        let _ = std::fs::remove_file(out);
        Ok(())
    }

    #[test]
    fn wrapper_output_matches_batch_packet_after_parser_facts() -> std::io::Result<()> {
        // Parity WITH real parser-backed facts: the wrapper writes exactly the
        // batch-API packet for a root that produces files + owners.
        let root = "target/ripr-p3-parity";
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(format!("{root}/lib"))?;
        std::fs::write(format!("{root}/lib/App.pm"), "package App;\nsub run { return 1; }\n1;\n")?;

        let out = format!("{root}/packet.json");
        let rc = run_ripr_facts("ripr-perl-facts-v1", root, None, None, "files,owners", &out);
        assert_eq!(rc, 0, "wrapper must succeed");
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out)?)?;

        let built = must_with(
            build_ripr_facts_packet(&RiprFactsRequest {
                schema: "ripr-perl-facts-v1",
                root,
                base: None,
                head: None,
                fact_classes: "files,owners",
                diff: None,
            }),
            "valid request",
        );
        // Sanity: the fixture actually yields owners, so parity covers PR-3 facts.
        assert!(!must_some(built["owners"].as_array()).is_empty(), "fixture must yield owners");
        assert_eq!(built, written, "wrapper output must equal the batch packet with parser facts");

        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn wrapper_output_matches_batch_packet_after_test_oracle_facts() -> std::io::Result<()> {
        // Parity means batch API == wrapper-written packet (not PR4 == PR3).
        let root = "target/ripr-p4-parity";
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(format!("{root}/t"))?;
        std::fs::write(
            format!("{root}/t/foo.t"),
            "use Test::More;\nis(1, 1, 'a');\nok(1, 'b');\n",
        )?;
        let out = format!("{root}/packet.json");
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            root,
            None,
            None,
            "tests,oracles,provenance,limitations",
            &out,
        );
        assert_eq!(rc, 0, "wrapper succeeds");
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out)?)?;
        let built = must_with(
            build_ripr_facts_packet(&RiprFactsRequest {
                schema: "ripr-perl-facts-v1",
                root,
                base: None,
                head: None,
                fact_classes: "tests,oracles,provenance,limitations",
                diff: None,
            }),
            "valid request",
        );
        assert_eq!(built, written, "batch API packet == wrapper-written packet");
        assert!(
            !must_some_with(built["oracles"].as_array(), "oracles[]").is_empty(),
            "oracles present"
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn ripr_facts_validates_schema_version() {
        let rc = run_ripr_facts(
            "wrong-schema",
            ".",
            None,
            None,
            "owners,changes",
            "target/ripr/test-wrong-schema.json",
        );
        assert_eq!(rc, 1, "wrong schema must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_absolute_root() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            "/absolute/path",
            None,
            None,
            "owners",
            "target/ripr/test-abs-root.json",
        );
        assert_eq!(rc, 1, "absolute root must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_path_escape() {
        let rc =
            run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "owners", "../../../etc/passwd");
        assert_eq!(rc, 1, "path escape must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_unknown_fact_class() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            ".",
            None,
            None,
            "owners,bogus_class",
            "target/ripr/test-bad-class.json",
        );
        assert_eq!(rc, 1, "unknown fact class must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_drive_path() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            "C:/repo",
            None,
            None,
            "owners",
            "target/ripr/test-drive.json",
        );
        assert_eq!(rc, 1, "Windows drive path must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_dot_slash_root() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            "./repo",
            None,
            None,
            "owners",
            "target/ripr/test-dot-slash.json",
        );
        assert_eq!(rc, 1, "./ prefix must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_empty_fact_classes() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            ".",
            None,
            None,
            "",
            "target/ripr/test-empty-classes.json",
        );
        assert_eq!(rc, 1, "empty fact_classes must exit 1");
    }

    #[test]
    fn ripr_facts_accepts_valid_invocation() {
        let out = "target/ripr/test-valid-invocation.json";
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            ".",
            Some("origin/main"),
            Some("HEAD"),
            "files,owners,changes,tests,oracles",
            out,
        );
        assert_eq!(rc, 0, "valid invocation must exit 0");
        let written = must_with(std::fs::read_to_string(out), "packet must be written");
        let parsed: serde_json::Value =
            must_with(serde_json::from_str(&written), "packet must be JSON");
        assert_eq!(parsed["packet_status"], "unavailable");
        // Clean up.
        let _ = std::fs::remove_file(out);
    }

    /// Call-observation test for the success-*with-facts* path.
    ///
    /// The other `run_ripr_facts` tests hit early-return validation (rc==1) or
    /// the empty-root success path (root=".", which finds no `.t` files and
    /// stays `unavailable`). This drives the full fact-producing chain end to
    /// end — the emitter discovers a real `.t` file, detects the framework,
    /// upgrades the packet to `partial`, and writes it — so the emitter seams
    /// are observed via a real call. It pairs with the string-scan ripr
    /// suppression in `policy/ripr-suppressions.toml` (ripr#1429 class): RIPR's
    /// static tracer cannot follow the string scans, but this observably
    /// exercises them.
    #[test]
    fn ripr_facts_success_with_test_facts_writes_partial_packet() -> std::io::Result<()> {
        // `run_ripr_facts` resolves `root`/`out` relative to the process CWD (the
        // crate dir under `cargo test`), so keep them repo-relative to pass the
        // path validator. A unique subdir avoids collision with the other tests.
        let root = "target/ripr-facts-selftest";
        let t_dir = format!("{root}/t");
        std::fs::create_dir_all(&t_dir)?;
        std::fs::write(
            format!("{t_dir}/basic.t"),
            "use Test::More;\nok(1, 'truthy');\nis(1, 1, 'one equals one');\ndone_testing;\n",
        )?;
        let out = format!("{root}/packet.json");

        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            root,
            None,
            None,
            "tests,oracles,relations,limitations",
            &out,
        );
        assert_eq!(rc, 0, "valid invocation with a .t file must exit 0");

        let written = std::fs::read_to_string(&out)?;
        let parsed: serde_json::Value = serde_json::from_str(&written)?;
        // Discovering a `.t` file upgrades the packet from `unavailable` to `partial`.
        assert_eq!(
            parsed["packet_status"], "partial",
            "a discovered .t file must yield a partial packet"
        );
        let tests = must_some_with(parsed["tests"].as_array(), "tests[] is an array");
        assert!(!tests.is_empty(), "the discovered .t file must produce a test fact");
        assert_eq!(
            tests[0]["framework"], "Test::More",
            "framework must be detected from `use Test::More`"
        );
        let capabilities = must_some_with(
            parsed["producer"]["capabilities"].as_array(),
            "capabilities[] is an array",
        );
        assert!(
            capabilities.iter().any(|capability| capability == "test_facts"),
            "packets carrying tests/oracles must advertise test_facts"
        );

        // Clean up the synthetic tree.
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn ripr_facts_deduplicates_and_orders_fact_classes() {
        let normalized = must_with(
            crate::request::normalize_fact_classes("changes,owners,owners,changes,tests"),
            "valid classes normalize",
        );
        // Canonical order (VALID_FACT_CLASSES order): files, owners, changes, tests, ...
        assert_eq!(normalized, vec!["owners", "changes", "tests"]);
    }

    #[test]
    fn ripr_facts_writes_unavailable_packet_to_disk() -> std::io::Result<()> {
        let out = "target/ripr/test-ripr-facts-write.json";
        let packet = build_unavailable_packet(
            "ripr-perl-facts-v1",
            ".",
            None,
            None,
            &["owners".to_string()],
        );
        write_packet(out, &packet)?;
        let written = std::fs::read_to_string(out)?;
        let parsed: serde_json::Value = serde_json::from_str(&written)?;
        assert_eq!(parsed["schema_version"], "ripr-perl-facts-v1");
        assert_eq!(parsed["packet_status"], "unavailable");
        Ok(())
    }

    #[test]
    fn ripr_facts_rejects_empty_root() {
        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            "",
            None,
            None,
            "owners",
            "target/ripr/test-empty-root.json",
        );
        assert_eq!(rc, 1, "empty root must exit 1");
    }

    #[test]
    fn ripr_facts_rejects_empty_out() {
        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "owners", "");
        assert_eq!(rc, 1, "empty out path must exit 1");
    }

    /// Build a packet over a fixture with `lib/App.pm` (a `sub discount`) and a
    /// caller-supplied `diff`, via the CLI diff-file path (`run_cli`'s
    /// `--diff <path>`). Local `changes_of`/`has_limitation` helpers mirror
    /// `crate::packet`'s test-only helpers of the same name (#9271 PR notes:
    /// duplicated rather than exposed cross-module).
    fn changes_of(p: &serde_json::Value) -> Vec<serde_json::Value> {
        must_some_with(p["changes"].as_array(), "changes[]").clone()
    }
    fn has_limitation(p: &serde_json::Value, id_prefix: &str) -> bool {
        must_some_with(p["limitations"].as_array(), "limitations[]")
            .iter()
            .any(|l| l["limitation_id"].as_str().is_some_and(|s| s.starts_with(id_prefix)))
    }

    /// A diff adding a line inside `sub discount` (0-based head line 3).
    const APP_DIFF: &str = "+++ b/lib/App.pm\n@@ -3,2 +3,3 @@\n     my ($amount) = @_;\n+    return $amount / 2;\n     return $amount;\n";

    #[test]
    fn ripr_facts_cli_reads_diff_file_and_emits_changes() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = format!("target/ripr-facts-cli-diff-{}", std::process::id());
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(format!("{root}/lib"))?;
        std::fs::write(
            format!("{root}/lib/App.pm"),
            "package App;\nsub discount {\n    my ($amount) = @_;\n    return $amount;\n}\n1;\n",
        )?;
        let diff_path = format!("{root}/diff.patch");
        std::fs::write(&diff_path, APP_DIFF)?;
        let out = format!("{root}/packet.json");

        let rc = run_cli(vec![
            "perl-ripr-facts".to_string(),
            "ripr-facts".to_string(),
            "--schema".to_string(),
            "ripr-perl-facts-v1".to_string(),
            "--root".to_string(),
            root.clone(),
            "--base".to_string(),
            "origin/main".to_string(),
            "--head".to_string(),
            "HEAD".to_string(),
            "--fact-classes".to_string(),
            "files,owners,changes".to_string(),
            "--diff".to_string(),
            diff_path,
            "--out".to_string(),
            out.clone(),
        ]);
        assert_eq!(rc, 0, "canonical CLI path should write a packet");

        let packet: serde_json::Value = serde_json::from_slice(&std::fs::read(&out)?)?;
        assert!(!changes_of(&packet).is_empty(), "diff file should populate changes[]");
        assert!(
            !has_limitation(&packet, "no-diff-supplied"),
            "a supplied diff file must not report no-diff-supplied"
        );

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn ripr_facts_cli_reads_diff_relative_to_process_cwd_not_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = format!("target/ripr-facts-cli-cwd-diff-{}", std::process::id());
        let root = format!("{base}/workspace");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(format!("{root}/lib"))?;
        std::fs::write(
            format!("{root}/lib/App.pm"),
            "package App;\nsub discount {\n    my ($amount) = @_;\n    return $amount;\n}\n1;\n",
        )?;
        let diff_path = format!("{base}/diff.patch");
        std::fs::write(&diff_path, APP_DIFF)?;
        let out = format!("{root}/packet.json");

        let rc = run_cli(vec![
            "perl-ripr-facts".to_string(),
            "ripr-facts".to_string(),
            "--schema".to_string(),
            "ripr-perl-facts-v1".to_string(),
            "--root".to_string(),
            root.clone(),
            "--base".to_string(),
            "origin/main".to_string(),
            "--head".to_string(),
            "HEAD".to_string(),
            "--fact-classes".to_string(),
            "files,owners,changes".to_string(),
            "--diff".to_string(),
            diff_path,
            "--out".to_string(),
            out.clone(),
        ]);
        assert_eq!(
            rc, 0,
            "managed producer --diff is repo/process-cwd relative, not --root relative"
        );

        let packet: serde_json::Value = serde_json::from_slice(&std::fs::read(&out)?)?;
        assert!(!changes_of(&packet).is_empty(), "diff file should populate changes[]");

        let _ = std::fs::remove_dir_all(&base);
        Ok(())
    }
    // ── #16022 / #8165 P2: atomic packet output ──
    //
    // The contract these prove: `write_packet` REPLACES the destination (stage
    // a sibling, then rename) instead of truncating it in place. The mutant to
    // kill is a revert to `std::fs::write(path, json)`. Two tests below fail
    // against that mutant — `write_replaces_the_destination_file_identity` and
    // `reader_holding_the_destination_open_still_sees_the_previous_packet`.
    // The rest guard the properties a correct rename must not lose: exact
    // bytes, no leftover scratch, and an untouched destination on failure.

    /// Build a Perl fixture root that yields real `files[]`/`owners[]` facts.
    fn perl_fixture(root: &str, body: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(format!("{root}/lib"))?;
        std::fs::write(format!("{root}/lib/App.pm"), body)
    }

    /// Names of the entries directly inside `dir`, sorted.
    fn dir_entries(dir: &str) -> std::io::Result<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<Vec<String>>>()?;
        names.sort();
        Ok(names)
    }

    #[test]
    fn write_replaces_the_whole_destination_and_preserves_exact_bytes() -> std::io::Result<()> {
        // A stale predecessor much LARGER than the new packet: if the write
        // only overwrote a prefix, the tail of the old file would survive and
        // the byte comparison below would fail.
        let dir = "target/ripr-atomic-replace";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let out = format!("{dir}/packet.json");
        std::fs::write(&out, "x".repeat(64 * 1024))?;

        let rc = run_ripr_facts(
            "ripr-perl-facts-v1",
            ".",
            Some("origin/main"),
            Some("HEAD"),
            "tests,oracles",
            &out,
        );
        assert_eq!(rc, 0, "wrapper must succeed");

        let written = std::fs::read_to_string(&out)?;
        let built = must_with(
            build_ripr_facts_packet(&valid_request("tests,oracles")),
            "valid request builds a packet",
        );
        let expected = serde_json::to_string_pretty(&built)?;

        // Byte identity, not JSON equality: this also pins the on-disk encoding
        // (pretty-printed, no trailing newline) so the atomic staging cannot
        // silently change the packet bytes RIPR consumers hash.
        assert_eq!(written, expected, "destination must hold exactly the serialized packet");
        assert!(!written.ends_with('\n'), "packet bytes carry no trailing newline");

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn successful_write_leaves_no_staged_sibling() -> std::io::Result<()> {
        let dir = "target/ripr-atomic-no-litter";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let out = format!("{dir}/packet.json");

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "wrapper must succeed");

        // Staging is an implementation detail that must not leak onto disk: a
        // `.packet.json.tmp-*` left behind would be picked up by directory
        // consumers and by the next run's cleanup assumptions.
        assert_eq!(
            dir_entries(dir)?,
            vec!["packet.json".to_string()],
            "only the packet may remain after a successful write"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn failed_packet_build_preserves_the_previous_packet_byte_for_byte() -> std::io::Result<()> {
        let dir = "target/ripr-atomic-build-failure";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let out = format!("{dir}/packet.json");

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "the first generation must succeed");
        let previous = std::fs::read(&out)?;
        assert!(!previous.is_empty(), "fixture must leave a real packet on disk");

        // A rejected request must not disturb the destination at all: validation
        // precedes every filesystem effect.
        let rc = run_ripr_facts("ripr-perl-facts-v2", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 1, "an unsupported schema must fail the run");
        assert_eq!(
            std::fs::read(&out)?,
            previous,
            "a failed generation must preserve the previous valid packet"
        );
        assert_eq!(
            dir_entries(dir)?,
            vec!["packet.json".to_string()],
            "a failed generation must not leave scratch behind either"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn failed_replace_reports_failure_and_removes_the_staged_sibling() -> std::io::Result<()> {
        // Occupy the destination with a directory. Staging succeeds, then the
        // rename fails (a file cannot replace a directory on Unix or Windows),
        // which exercises the replace-failure path end to end.
        let dir = "target/ripr-atomic-replace-failure";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(format!("{dir}/packet.json"))?;
        let out = format!("{dir}/packet.json");

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 1, "an unusable destination must fail the run");
        assert_eq!(
            dir_entries(dir)?,
            vec!["packet.json".to_string()],
            "a failed replace must not leave the staged sibling behind"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    // The destination-identity and open-handle proofs below read the inode the
    // path resolves to, which has no portable equivalent: Windows file-id reads
    // need `std::os::windows::fs::FileExt`-adjacent APIs that are not stable,
    // and replacement semantics for a held handle differ there. The property
    // itself is platform-independent; only this instrument is Unix-only.
    #[cfg(unix)]
    #[test]
    fn write_replaces_the_destination_file_identity() -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt as _;

        let dir = "target/ripr-atomic-identity";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let out = format!("{dir}/packet.json");

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "the first generation must succeed");
        let first = std::fs::metadata(&out)?.ino();

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "the second generation must succeed");
        let second = std::fs::metadata(&out)?.ino();

        // NEGATIVE CONTROL. `std::fs::write` truncates and rewrites the SAME
        // inode, so it keeps this identity stable; a staged-sibling rename
        // always publishes a new one. Reverting `write_packet` to
        // `std::fs::write` fails exactly here.
        assert_ne!(
            first, second,
            "the destination must be replaced by rename, not truncated in place"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn reader_holding_the_destination_open_still_sees_the_previous_packet()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Read as _;

        let dir = "target/ripr-atomic-open-handle";
        let _ = std::fs::remove_dir_all(dir);
        let root = format!("{dir}/root");
        let out = format!("{dir}/out/packet.json");
        perl_fixture(&root, "package App;\nsub run { return 1; }\n1;\n")?;

        let rc = run_ripr_facts("ripr-perl-facts-v1", &root, None, None, "files,owners", &out);
        assert_eq!(rc, 0, "the first generation must succeed");
        let previous = std::fs::read_to_string(&out)?;

        // A consumer that opened the packet just before the producer ran.
        let mut held = std::fs::File::open(&out)?;

        // Regenerate from changed sources so the new packet genuinely differs.
        perl_fixture(&root, "package App;\nsub run { return 1; }\nsub extra { return 2; }\n1;\n")?;
        let rc = run_ripr_facts("ripr-perl-facts-v1", &root, None, None, "files,owners", &out);
        assert_eq!(rc, 0, "the second generation must succeed");
        let current = std::fs::read_to_string(&out)?;
        assert_ne!(current, previous, "fixture must actually change the packet");

        let mut observed = String::new();
        held.read_to_string(&mut observed)?;

        // NEGATIVE CONTROL. Truncate-in-place mutates the bytes this handle
        // points at, so `std::fs::write` would yield the NEW packet (or a
        // truncated prefix) here. A rename leaves the old file intact until the
        // last reader closes it, which is precisely "a failed or concurrent
        // generation never corrupts a reader's packet".
        assert_eq!(
            observed, previous,
            "an open reader must keep observing the complete previous packet"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }
    // ── #16022 review round 2: behavior drift vs the old `std::fs::write` ──
    //
    // Replacing a file is not the same operation as rewriting one, and three
    // differences are observable. These pin the ones that would otherwise be
    // silent regressions against `std::fs::write`.

    #[test]
    fn staged_sibling_always_lands_beside_its_destination() {
        // The EXDEV guarantee the rustdoc and README both claim. An
        // implementation that staged into `std::env::temp_dir()` and renamed
        // across mounts passes every other test in this file — the success and
        // failure cleanup tests only assert the destination directory holds no
        // scratch, which is trivially true if the scratch was never put there.
        // Only this test fails against that wrong implementation.
        for destination in ["target/ripr-x/packet.json", "packet.json", "/tmp/packet.json"] {
            let path = std::path::Path::new(destination);
            let staged = temp_sibling_path(path);
            assert_eq!(
                staged.parent(),
                path.parent(),
                "staged sibling for `{destination}` must share the destination's directory"
            );
            assert_ne!(staged, path, "staged sibling must not be the destination itself");
        }
    }

    #[test]
    fn staged_sibling_names_are_unique_per_call() {
        // Two writers aimed at one destination must not share a scratch path.
        let path = std::path::Path::new("target/ripr-x/packet.json");
        assert_ne!(temp_sibling_path(path), temp_sibling_path(path));
    }

    #[cfg(unix)]
    #[test]
    fn write_preserves_an_existing_destinations_permissions() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        // `std::fs::write` reuses the destination inode, so a `chmod 600` on
        // the packet survived a regeneration. A staged sibling is a new file,
        // so without carrying the mode across it would publish at the umask
        // default and silently widen an operator's restriction on every run.
        let dir = "target/ripr-atomic-perms";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let out = format!("{dir}/packet.json");

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "the first generation must succeed");
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o600))?;

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &out);
        assert_eq!(rc, 0, "the second generation must succeed");

        assert_eq!(
            std::fs::metadata(&out)?.permissions().mode() & 0o777,
            0o600,
            "regenerating the packet must not widen the destination's permissions"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn write_through_a_symlinked_destination_keeps_the_link_and_updates_its_target()
    -> Result<(), Box<dyn std::error::Error>> {
        // `std::fs::write` follows a symlinked destination and updates the file
        // it names. A bare rename onto the link would replace the link with a
        // regular file and strand the real target at its previous contents —
        // silently breaking a `latest.json -> packet-vN.json` publication.
        let dir = "target/ripr-atomic-symlink";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(format!("{dir}/real"))?;
        let target = format!("{dir}/real/actual.json");
        std::fs::write(&target, "{}")?;

        let link = format!("{dir}/packet.json");
        std::os::unix::fs::symlink("real/actual.json", &link)?;

        let rc = run_ripr_facts("ripr-perl-facts-v1", ".", None, None, "tests,oracles", &link);
        assert_eq!(rc, 0, "writing through the link must succeed");

        assert!(
            std::fs::symlink_metadata(&link)?.file_type().is_symlink(),
            "the destination must still be a symlink, not a replaced regular file"
        );
        let through_link = std::fs::read_to_string(&link)?;
        let at_target = std::fs::read_to_string(&target)?;
        assert_eq!(through_link, at_target, "the link must still name the written file");
        assert_ne!(at_target, "{}", "the real target must have been updated, not stranded");

        // Atomicity still holds for the resolved file: staging happened beside
        // the target, so nothing was left behind in either directory.
        assert_eq!(
            dir_entries(&format!("{dir}/real"))?,
            vec!["actual.json".to_string()],
            "no staged sibling may remain beside the resolved target"
        );

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn resolve_destination_returns_a_plain_path_unchanged() {
        // Negative control for the symlink walk: a non-link destination, and a
        // destination that does not exist yet, must resolve to themselves
        // rather than to some canonicalized or parent-relative rewrite.
        let plain = std::path::Path::new("target/ripr-x/packet.json");
        assert_eq!(resolve_destination(plain), plain.to_path_buf());

        let missing = std::path::Path::new("target/ripr-x/does-not-exist/packet.json");
        assert_eq!(resolve_destination(missing), missing.to_path_buf());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_destination_terminates_on_a_symlink_cycle() {
        // A cycle must return rather than spin; the bounded walk is what makes
        // that true, and the returned path is then reported by the ordinary
        // create path as a real error instead of hanging the producer.
        let dir = "target/ripr-atomic-cycle";
        let _ = std::fs::remove_dir_all(dir);
        let Ok(()) = std::fs::create_dir_all(dir) else { return };
        let a = format!("{dir}/a");
        let b = format!("{dir}/b");
        let Ok(()) = std::os::unix::fs::symlink("b", &a) else { return };
        let Ok(()) = std::os::unix::fs::symlink("a", &b) else { return };

        let resolved = resolve_destination(std::path::Path::new(&a));
        assert!(resolved.ends_with("a") || resolved.ends_with("b"), "cycle must terminate");

        let _ = std::fs::remove_dir_all(dir);
    }

    // #16162 — exclusive allocation of the staged sibling. A stale scratch file
    // left behind by a previous interrupted run must NOT be truncated by
    // `File::create`; staging must claim a fresh sibling via O_EXCL semantics
    // and retry until either a fresh slot is found or the bounded attempts
    // are exhausted.

    #[test]
    fn stage_temp_sibling_preserves_an_existing_scratch_file() -> std::io::Result<()> {
        // A pre-existing scratch file with the next sequence number's name must
        // remain byte-identical: `stage_temp_sibling` allocates a fresh sibling
        // rather than truncating the one it found.
        let dir = "target/ripr-exclusive-claim";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        let dest_string = format!("{dir}/packet.json");
        let dest = std::path::Path::new(&dest_string);

        // Predict the sibling name the implementation will compute first, plant
        // a sentinel at exactly that path, and verify the sentinel survives.
        let claimed_first = temp_sibling_path(dest);
        let sentinel = b"PRE-EXISTING-SIBLING-DO-NOT-TRUNCATE";
        std::fs::write(&claimed_first, sentinel)?;

        let written = stage_temp_sibling(dest, b"new packet bytes")?;
        assert_ne!(
            written, claimed_first,
            "staging must allocate a sibling distinct from the pre-existing scratch",
        );

        let preserved = std::fs::read(&claimed_first)?;
        assert_eq!(
            preserved, sentinel,
            "the pre-existing sibling must be preserved byte-for-byte, not truncated",
        );

        let new_bytes = std::fs::read(&written)?;
        assert_eq!(new_bytes, b"new packet bytes");

        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }

    #[test]
    fn stage_temp_sibling_returns_a_path_we_own() {
        // The returned path is the one the implementation actually created;
        // the caller uses it to clean up only its own scratch on rename
        // failure. A test that fails on a wrong return value would silently
        // widen cleanup to a file we did not create.
        let dir = "target/ripr-owned-return";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        let dest_string = format!("{dir}/packet.json");
        let dest = std::path::Path::new(&dest_string);

        let returned = stage_temp_sibling(dest, b"payload").unwrap();
        assert!(
            returned.starts_with(dir),
            "returned sibling must live in the destination's directory: got {}",
            returned.display(),
        );
        assert!(returned.exists(), "the returned sibling must exist on disk");
        assert!(
            returned
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.') && name.contains(".tmp-")),
            "the returned sibling must match the dot-prefixed `.tmp-{{pid}}-{{seq}}` scheme",
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stage_temp_sibling_propagates_unrelated_io_errors() {
        // When the bounded retry loop hits a non-collision error, that error
        // is surfaced verbatim. The destination is not created, and no
        // scratch is left behind.
        //
        // We trigger `NotADirectory` by passing a destination whose parent
        // is a regular file: `OpenOptions::create_new` cannot create a
        // sibling next to a regular file when the parent component is
        // expected to be a directory.
        let dir = "target/ripr-non-collision";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/regular-file"), b"not a directory").unwrap();

        let blocker = format!("{dir}/regular-file/packet.json");
        let dest = std::path::Path::new(&blocker);
        let result = stage_temp_sibling(dest, b"payload");
        let err = result.expect_err("a non-directory parent must surface an error");
        assert_ne!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists,
            "a non-collision error must not be misclassified as `AlreadyExists`",
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finish_staged_file_removes_owned_scratch_on_write_failure() {
        // A staging write that fails after exclusive creation must not leave
        // a partial sibling behind: every failed attempt would otherwise burn
        // a sequence slot and accumulate scratch that a later run must treat
        // as a stranger. A read-only handle fails the write deterministically
        // on every platform without fault injection.
        let dir = "target/ripr-failed-stage-cleanup";
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        let owned_string = format!("{dir}/owned.tmp");
        let owned = std::path::Path::new(&owned_string);
        std::fs::write(owned, b"stale").unwrap();

        let read_only = std::fs::File::open(owned).unwrap();
        let err = finish_staged_file(read_only, owned, b"new bytes")
            .expect_err("writing through a read-only handle must fail");
        assert_ne!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists,
            "a failed staged write is not a collision to be retried",
        );
        assert!(
            !owned.exists(),
            "failed staging must remove the owned scratch instead of leaving a partial file",
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
