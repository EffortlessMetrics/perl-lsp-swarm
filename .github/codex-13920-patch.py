from pathlib import Path

path = Path("xtask/src/tasks/rust_small_proof.rs")
source = path.read_text()

source = source.replace(
    "/// profile drift fails `cargo test -p xtask` before it can reach CI.\n",
    "/// profile drift fails `cargo test -p xtask` before it can reach CI. The\n/// linked-list target has an explicit census plus replay so compiling a new\n/// assertion can never be mistaken for executing it (#13920).\n",
    1,
)

constants_anchor = "\n/// References scorecard census argv: replaces both old per-route counters with\n"
linked_constants = r'''
/// Linked-list receiver census argv. The separate list and replay commands make
/// assertion execution observable and fail closed if the target is renamed,
/// filtered away, or reduced to an empty binary (#13920).
const LINKED_LIST_CENSUS_ARGS: &[&str] = &[
    "test",
    "-p",
    "perl-semantic-analyzer",
    "--test",
    "linked_list_receiver_facts",
    "--locked",
    "--",
    "--list",
];

const LINKED_LIST_RUN_ARGS: &[&str] = &[
    "test",
    "-p",
    "perl-semantic-analyzer",
    "--test",
    "linked_list_receiver_facts",
    "--locked",
    "--",
    "--test-threads=1",
];
'''
if source.count(constants_anchor) != 1:
    raise SystemExit("expected one scorecard constants anchor")
source = source.replace(constants_anchor, "\n" + linked_constants + constants_anchor, 1)

run_start = source.index("pub fn run() -> Result<()> {")
diff_marker = source.index("    // Diff hygiene copied", run_start)
new_run_prefix = r'''pub fn run() -> Result<()> {
    let total = CARGO_STEPS.len() + 5;
    let mut done = 0usize;
    for (name, args) in CARGO_STEPS {
        run_cargo(name, args)?;
        done += 1;
    }

    run_cargo_census("linked-list receiver census", LINKED_LIST_CENSUS_ARGS, done, total)?;
    done += 1;

    run_cargo("linked-list receiver replay", LINKED_LIST_RUN_ARGS)?;
    done += 1;

    run_cargo_census("references scorecard census", SCORECARD_CENSUS_ARGS, done, total)?;
    done += 1;

    run_step("cargo", "references scorecard replay", SCORECARD_RUN_ARGS, SCORECARD_REPLAY_ENV)?;
    done += 1;

'''
source = source[:run_start] + new_run_prefix + source[diff_marker:]

helper_anchor = r'''fn run_cargo(name: &str, args: &[&str]) -> Result<()> {
    run_step("cargo", name, args, [])
}
'''
census_helper = r'''

fn run_cargo_census(name: &str, args: &[&str], done: usize, total: usize) -> Result<usize> {
    println!("[rust-small-proof] $ cargo {}", args.join(" "));
    let output = Command::new("cargo").args(args).output().map_err(|error| {
        eyre!("[{done}/{total}] {name} instrument failure (spawn): {error}")
    })?;
    if !output.status.success() {
        bail!(
            "[{done}/{total}] {name} product/test failure (exit {:?}): {}",
            output.status.code(),
            bounded_stderr(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let census = count_libtest_listed_tests(&stdout);
    println!("[{done}/{total}] {name}: {census} tests");
    if census == 0 {
        bail!(
            "[{done}/{total}] {name} is zero; the required proof vanished from \
             the listing — refusing to pass an empty gate"
        );
    }
    Ok(census)
}
'''
if source.count(helper_anchor) != 1:
    raise SystemExit("expected one run_cargo helper anchor")
source = source.replace(helper_anchor, helper_anchor + census_helper, 1)

test_anchor = r'''    #[test]
    fn scorecard_steps_pin_candidate_profile_feature_lock() {
'''
linked_test = r'''    #[test]
    fn linked_list_steps_pin_executed_target_and_census() {
        assert_eq!(
            LINKED_LIST_CENSUS_ARGS,
            &[
                "test",
                "-p",
                "perl-semantic-analyzer",
                "--test",
                "linked_list_receiver_facts",
                "--locked",
                "--",
                "--list",
            ]
        );
        assert_eq!(
            LINKED_LIST_RUN_ARGS,
            &[
                "test",
                "-p",
                "perl-semantic-analyzer",
                "--test",
                "linked_list_receiver_facts",
                "--locked",
                "--",
                "--test-threads=1",
            ]
        );
        assert!(LINKED_LIST_CENSUS_ARGS.contains(&"--list"));
        assert!(!LINKED_LIST_RUN_ARGS.contains(&"--list"));
        assert!(!LINKED_LIST_RUN_ARGS.contains(&"--no-run"));
    }

'''
if source.count(test_anchor) != 1:
    raise SystemExit("expected one scorecard test anchor")
source = source.replace(test_anchor, linked_test + test_anchor, 1)

path.write_text(source)
Path(".github/workflows/codex-13920-patch.yml").unlink()
Path(".github/codex-13920-patch.py").unlink()
