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
fn write_packet(out: &str, packet: &serde_json::Value) -> std::io::Result<()> {
    let path = std::path::Path::new(out);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(packet)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::build_unavailable_packet;
    use perl_tdd_support::must_some;

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
        let built = build_ripr_facts_packet(&valid_request("tests,oracles"))
            .expect("valid request builds a packet");
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

        let built = build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root,
            base: None,
            head: None,
            fact_classes: "files,owners",
            diff: None,
        })
        .expect("valid request");
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
        let built = build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root,
            base: None,
            head: None,
            fact_classes: "tests,oracles,provenance,limitations",
            diff: None,
        })
        .expect("valid request");
        assert_eq!(built, written, "batch API packet == wrapper-written packet");
        assert!(!built["oracles"].as_array().expect("oracles[]").is_empty(), "oracles present");
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
        let written = std::fs::read_to_string(out).expect("packet must be written");
        let parsed: serde_json::Value =
            serde_json::from_str(&written).expect("packet must be JSON");
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
        let tests = parsed["tests"].as_array().expect("tests[] is an array");
        assert!(!tests.is_empty(), "the discovered .t file must produce a test fact");
        assert_eq!(
            tests[0]["framework"], "Test::More",
            "framework must be detected from `use Test::More`"
        );
        let capabilities =
            parsed["producer"]["capabilities"].as_array().expect("capabilities[] is an array");
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
        let normalized =
            crate::request::normalize_fact_classes("changes,owners,owners,changes,tests")
                .expect("valid classes normalize");
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
        p["changes"].as_array().expect("changes[]").clone()
    }
    fn has_limitation(p: &serde_json::Value, id_prefix: &str) -> bool {
        p["limitations"]
            .as_array()
            .expect("limitations[]")
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
}
