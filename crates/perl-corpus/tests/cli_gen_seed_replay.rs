use perl_tdd_support::{must_some_with, must_with};
use std::process::{Command, Output};

fn run_gen(args: &[&str]) -> Output {
    must_with(
        Command::new(env!("CARGO_BIN_EXE_perl-corpus")).args(args).output(),
        format!("perl-corpus {args:?} should spawn"),
    )
}

fn stdout(output: &Output) -> String {
    must_with(
        std::str::from_utf8(&output.stdout).map(|s| s.to_string()),
        "perl-corpus stdout should be UTF-8",
    )
}

fn body_without_seed_header(output: &Output) -> String {
    stdout(output)
        .lines()
        .filter(|line| !line.starts_with("# Generated with perl-corpus"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "perl-corpus should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn gen_fixed_seed_replays_identically_in_separate_processes() {
    let first = run_gen(&["gen", "--seed", "42", "--count", "3", "qw"]);
    let second = run_gen(&["gen", "--seed", "42", "--count", "3", "qw"]);
    assert_success(&first);
    assert_success(&second);
    assert!(stdout(&first).contains("(seed: 42)"));
    assert!(stdout(&first).contains("# Test case 3 (qw)"));
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn gen_different_seeds_produce_different_bodies() {
    let seed42 = run_gen(&["gen", "--seed", "42", "--count", "3", "qw"]);
    let seed43 = run_gen(&["gen", "--seed", "43", "--count", "3", "qw"]);
    assert_success(&seed42);
    assert_success(&seed43);
    assert_ne!(body_without_seed_header(&seed42), body_without_seed_header(&seed43));
}

#[test]
fn gen_zero_and_full_width_seeds_replay_identically() {
    for seed in ["0", "18446744073709551615"] {
        let first = run_gen(&["gen", "--seed", seed, "--count", "3", "qw"]);
        let second = run_gen(&["gen", "--seed", seed, "--count", "3", "qw"]);
        assert_success(&first);
        assert_success(&second);
        assert!(stdout(&first).contains(&format!("(seed: {seed})")));
        assert_eq!(stdout(&first), stdout(&second));
    }
}

#[test]
fn gen_program_fixed_seed_replays_identically() {
    let first = run_gen(&["gen", "--seed", "42", "--count", "3", "program"]);
    let second = run_gen(&["gen", "--seed", "42", "--count", "3", "program"]);
    assert_success(&first);
    assert_success(&second);
    assert!(stdout(&first).contains("(seed: 42)"));
    assert!(stdout(&first).contains("# Program (3 statements)"));
    assert_eq!(stdout(&first), stdout(&second));
    let expected = perl_corpus::generate_perl_code_with_seed(3, 42);
    let text = stdout(&first);
    let marker = "# Program (3 statements)\n";
    let (_, body) =
        must_some_with(text.split_once(marker), "CLI output should contain program marker");
    assert_eq!(body, format!("{expected}\n"));
}

#[test]
fn gen_zero_count_succeeds_without_cases() {
    let output = run_gen(&["gen", "--seed", "42", "--count", "0", "qw"]);
    assert_success(&output);
    assert!(stdout(&output).contains("(seed: 42)"));
    assert!(!stdout(&output).contains("# Test case"));
}

#[test]
fn gen_without_seed_uses_convenience_seed_and_succeeds() {
    let output = run_gen(&["gen", "--count", "2", "qw"]);
    assert_success(&output);
    assert!(stdout(&output).contains("# Test case 2 (qw)"));
}
