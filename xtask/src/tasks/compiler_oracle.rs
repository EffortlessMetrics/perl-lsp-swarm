//! Differential proof helpers for compiler-substrate facts.
//!
//! This module keeps real Perl as a conformance oracle for selected bounded
//! facts. It is not used by LSP providers.

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result, bail};
    use perl_parser_core::Parser;
    use perl_parser_core::hir::{CompileEffect, CompileEffectKind, HirFile, lower_ast};
    use serde::Serialize;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, ExitStatus, Output, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const FIXTURE_ID: &str = "compile_effect_basic_stash_facts";
    const COMPARED_FAMILIES: [&str; 5] = ["package", "sub", "constant", "prototype", "isa"];
    const EXPECTED_FIXTURE_FACTS: [(&str, &str); 8] = [
        ("package", "Oracle::Base"),
        ("package", "Oracle::Demo"),
        ("sub", "Oracle::Base::inherited"),
        ("sub", "Oracle::Demo::ordinary"),
        ("sub", "Oracle::Demo::proto"),
        ("prototype", "Oracle::Demo::proto"),
        ("constant", "Oracle::Demo::ANSWER"),
        ("isa", "Oracle::Demo->Oracle::Base"),
    ];
    const ORACLE_TIMEOUT: Duration = Duration::from_secs(10);
    const ORACLE_POLL_INTERVAL: Duration = Duration::from_millis(10);
    const FIXTURE_SOURCE: &str = r#"
package Oracle::Base;
sub inherited { 1 }

package Oracle::Demo;
our @ISA = qw(Oracle::Base);
use constant ANSWER => 42;
sub proto ($) { 1 }
sub ordinary { 1 }
1;
"#;

    const ORACLE_PROBE: &str = r#"
use strict;
use warnings;

my $file = shift @ARGV;
my $loaded = do $file;
if (!$loaded) {
    die $@ || $!;
}

for my $package (qw(Oracle::Base Oracle::Demo)) {
    no strict 'refs';
    print "package\t$package\n" if scalar keys %{"${package}::"};
}

for my $sub (qw(Oracle::Base::inherited Oracle::Demo::proto Oracle::Demo::ordinary)) {
    no strict 'refs';
    print "sub\t$sub\n" if defined &{$sub};
}

for my $sub (qw(Oracle::Demo::proto)) {
    my $prototype = prototype($sub);
    print "prototype\t$sub\n" if defined $prototype;
}

{
    no strict 'refs';
    for my $parent (@{"Oracle::Demo::ISA"}) {
        print "isa\tOracle::Demo->$parent\n";
    }
}

{
    no strict 'refs';
    my $constant = "Oracle::Demo::ANSWER";
    print "constant\t$constant\n" if defined &{$constant} && defined prototype($constant);
}
"#;

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
    struct NormalizedFact {
        family: &'static str,
        name: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    struct FactDisagreement {
        family: &'static str,
        name: String,
        side: &'static str,
    }

    #[derive(Debug, Clone, Serialize)]
    struct DifferentialReceipt {
        fixture_id: &'static str,
        perl_version: String,
        compared_families: Vec<&'static str>,
        matched_facts: Vec<NormalizedFact>,
        disagreements: Vec<FactDisagreement>,
    }

    #[test]
    fn compiler_compile_effect_oracle_compares_selected_facts() -> Result<()> {
        let hir = lower_source(FIXTURE_SOURCE);
        let rust_facts = normalize_rust_compile_effects(&hir);
        let observed = run_perl_oracle(FIXTURE_SOURCE)?;
        let expected_facts = expected_fixture_facts();

        assert_eq!(
            rust_facts, expected_facts,
            "the Rust side must match the expected fixture fact manifest"
        );
        assert_eq!(
            observed.facts, expected_facts,
            "the Perl oracle must match the expected fixture fact manifest"
        );

        let receipt = compare_facts(observed.perl_version, rust_facts, observed.facts);
        let rendered = serde_json::to_string_pretty(&receipt)
            .context("serialize compile-effect oracle receipt")?;
        println!("{rendered}");

        assert!(
            receipt.disagreements.is_empty(),
            "compile-effect oracle disagreements: {rendered}"
        );
        assert_eq!(
            receipt.compared_families,
            vec!["package", "sub", "constant", "prototype", "isa"],
            "receipt should cover exactly the selected bounded fact families"
        );
        assert_eq!(
            receipt.matched_facts.iter().cloned().collect::<BTreeSet<_>>(),
            expected_facts,
            "the differential receipt must not pass with an empty or partial intersection"
        );

        Ok(())
    }

    struct PerlOracleOutput {
        perl_version: String,
        facts: BTreeSet<NormalizedFact>,
    }

    fn lower_source(source: &str) -> HirFile {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        lower_ast(&output.ast)
    }

    fn normalize_rust_compile_effects(file: &HirFile) -> BTreeSet<NormalizedFact> {
        file.compile_effects().iter().filter_map(normalize_rust_compile_effect).collect()
    }

    fn expected_fixture_facts() -> BTreeSet<NormalizedFact> {
        EXPECTED_FIXTURE_FACTS
            .into_iter()
            .map(|(family, name)| normalized(family, name.to_string()))
            .collect()
    }

    fn normalize_rust_compile_effect(effect: &CompileEffect) -> Option<NormalizedFact> {
        match effect.kind {
            CompileEffectKind::DeclarePackage => {
                effect.fact_name.as_ref().map(|name| normalized("package", name.clone()))
            }
            CompileEffectKind::DeclareSub => effect.fact_name.as_ref().map(|name| {
                normalized("sub", qualify_name(effect.package_context.as_deref(), name))
            }),
            CompileEffectKind::AssignInheritance => {
                effect.fact_name.as_ref().map(|name| normalized("isa", name.clone()))
            }
            CompileEffectKind::DefineConstant => {
                effect.fact_name.as_ref().map(|name| normalized("constant", name.clone()))
            }
            CompileEffectKind::RegisterPrototype => effect.fact_name.as_ref().map(|name| {
                normalized("prototype", qualify_name(effect.package_context.as_deref(), name))
            }),
            _ => None,
        }
    }

    fn run_perl_oracle(source: &str) -> Result<PerlOracleOutput> {
        let tempdir = tempfile::tempdir().context("create compiler-oracle fixture tempdir")?;
        let fixture_path = tempdir.path().join("OracleDemo.pm");
        fs::write(&fixture_path, source).context("write compiler-oracle fixture")?;

        let perl_version = query_perl_version()?;
        let mut command = isolated_perl_command();
        command.arg("-I").arg(tempdir.path()).arg("-e").arg(ORACLE_PROBE).arg(&fixture_path);
        let output =
            run_bounded_command(&mut command, ORACLE_TIMEOUT, "Perl compile-effect oracle probe")?;

        if !output.status.success() {
            bail!(
                "Perl compile-effect oracle failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout =
            String::from_utf8(output.stdout).context("decode Perl compile-effect oracle stdout")?;
        Ok(PerlOracleOutput { perl_version, facts: parse_oracle_facts(&stdout)? })
    }

    fn query_perl_version() -> Result<String> {
        let mut command = isolated_perl_command();
        command.arg("-e").arg("print $^V");
        let output = run_bounded_command(
            &mut command,
            ORACLE_TIMEOUT,
            "Perl version probe for compile-effect oracle",
        )?;

        if !output.status.success() {
            bail!(
                "Perl version probe failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8(output.stdout).context("decode Perl version output")
    }

    fn isolated_perl_command() -> Command {
        let mut command = Command::new("perl");
        command.env_remove("PERL5OPT").env_remove("PERL5LIB").env("LC_ALL", "C");
        command
    }

    fn run_bounded_command(
        command: &mut Command,
        timeout: Duration,
        operation: &str,
    ) -> Result<Output> {
        run_bounded_command_with_poll(command, timeout, operation, |child| child.try_wait())
    }

    fn run_bounded_command_with_poll(
        command: &mut Command,
        timeout: Duration,
        operation: &str,
        mut poll: impl FnMut(&mut Child) -> std::io::Result<Option<ExitStatus>>,
    ) -> Result<Output> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn {operation}"))?;
        let started = Instant::now();

        loop {
            let status = match poll(&mut child) {
                Ok(status) => status,
                Err(error) => {
                    return Err(cleanup_after_poll_error(child, error, operation));
                }
            };
            if status.is_some() {
                return child
                    .wait_with_output()
                    .with_context(|| format!("collect {operation} output"));
            }

            let elapsed = started.elapsed();
            if elapsed >= timeout {
                if let Err(error) = child.kill() {
                    if child
                        .try_wait()
                        .with_context(|| format!("poll {operation} after kill failure"))?
                        .is_none()
                    {
                        return Err(error).with_context(|| format!("kill timed-out {operation}"));
                    }
                }
                let output = child
                    .wait_with_output()
                    .with_context(|| format!("reap timed-out {operation}"))?;
                bail!(
                    "{operation} timed out after {} ms; stderr: {}",
                    timeout.as_millis(),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }

            thread::sleep(ORACLE_POLL_INTERVAL.min(timeout.saturating_sub(elapsed)));
        }
    }

    fn cleanup_after_poll_error(
        mut child: Child,
        poll_error: std::io::Error,
        operation: &str,
    ) -> anyhow::Error {
        let kill_error = child.kill().err();
        let reap_error = if kill_error.is_none() {
            child.wait_with_output().err()
        } else {
            match child.try_wait() {
                Ok(Some(_)) => child.wait_with_output().err(),
                Ok(None) | Err(_) => None,
            }
        };

        let mut details = format!("{operation} polling failed: {poll_error}");
        if let Some(error) = kill_error {
            details.push_str(&format!("; kill cleanup failed: {error}"));
        } else {
            details.push_str("; child kill requested");
        }
        if let Some(error) = reap_error {
            details.push_str(&format!("; reap cleanup failed: {error}"));
        } else {
            details.push_str("; child reap attempted");
        }
        anyhow::anyhow!(details)
    }

    fn parse_oracle_facts(stdout: &str) -> Result<BTreeSet<NormalizedFact>> {
        let mut facts = BTreeSet::new();
        for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
            let mut parts = line.split('\t');
            let family = parts
                .next()
                .filter(|value| !value.is_empty())
                .context("Perl oracle line missing fact family")?;
            let name = parts
                .next()
                .filter(|value| !value.is_empty())
                .context("Perl oracle line missing fact name")?;
            if parts.next().is_some() {
                bail!("Perl oracle line has extra fields: {line}");
            }
            let family = match family {
                "package" => "package",
                "sub" => "sub",
                "constant" => "constant",
                "prototype" => "prototype",
                "isa" => "isa",
                other => bail!("unknown Perl oracle fact family: {other}"),
            };
            let fact = normalized(family, name.to_string());
            if !facts.insert(fact.clone()) {
                bail!("duplicate Perl oracle fact: {}\t{}", fact.family, fact.name);
            }
        }
        if facts.is_empty() {
            bail!("Perl oracle emitted no recognized facts");
        }
        Ok(facts)
    }

    fn compare_facts(
        perl_version: String,
        rust_facts: BTreeSet<NormalizedFact>,
        perl_facts: BTreeSet<NormalizedFact>,
    ) -> DifferentialReceipt {
        let matched_facts =
            rust_facts.intersection(&perl_facts).cloned().collect::<Vec<NormalizedFact>>();
        let missing_in_perl = rust_facts
            .difference(&perl_facts)
            .map(|fact| disagreement(fact.family, fact.name.clone(), "rust_only"));
        let missing_in_rust = perl_facts
            .difference(&rust_facts)
            .map(|fact| disagreement(fact.family, fact.name.clone(), "perl_only"));
        let disagreements = missing_in_perl.chain(missing_in_rust).collect::<Vec<_>>();

        DifferentialReceipt {
            fixture_id: FIXTURE_ID,
            perl_version,
            compared_families: COMPARED_FAMILIES.to_vec(),
            matched_facts,
            disagreements,
        }
    }

    fn normalized(family: &'static str, name: String) -> NormalizedFact {
        NormalizedFact { family, name }
    }

    fn disagreement(family: &'static str, name: String, side: &'static str) -> FactDisagreement {
        FactDisagreement { family, name, side }
    }

    fn qualify_name(package: Option<&str>, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        match package {
            Some(package) if !package.is_empty() => format!("{package}::{name}"),
            _ => name.to_string(),
        }
    }

    #[test]
    fn compiler_oracle_timeout_is_bounded_and_retains_stderr() -> Result<()> {
        let mut command = isolated_perl_command();
        command.arg("-e").arg(
            r#"print STDERR "compiler-oracle-timeout-sentinel\n"; select undef, undef, undef, 2;"#,
        );

        let started = Instant::now();
        let error = run_bounded_command(
            &mut command,
            Duration::from_millis(250),
            "compiler-oracle timeout falsifier",
        )
        .err()
        .context("sleeping Perl probe should exceed the compiler-oracle deadline")?;
        let message = format!("{error:#}");

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout should return near the deadline rather than after the full sleep"
        );
        assert!(
            message.contains("timed out after 250 ms"),
            "timeout should remain explicit: {message}"
        );
        assert!(
            message.contains("compiler-oracle-timeout-sentinel"),
            "timeout should retain child stderr: {message}"
        );
        Ok(())
    }

    #[test]
    fn compiler_oracle_poll_failure_cleans_up_child() -> Result<()> {
        let mut command = isolated_perl_command();
        command
            .arg("-e")
            .arg(r#"select undef, undef, undef, 2;"#);

        let error = run_bounded_command_with_poll(
            &mut command,
            Duration::from_secs(1),
            "compiler-oracle poll failure falsifier",
            |_| Err(std::io::Error::other("synthetic poll failure")),
        )
        .err()
        .context("synthetic poll failure should be surfaced")?;
        let message = format!("{error:#}");

        assert!(
            message.contains("compiler-oracle poll failure falsifier polling failed")
                && message.contains("synthetic poll failure"),
            "the original polling error must remain visible: {message}"
        );
        assert!(
            message.contains("child kill requested")
                && message.contains("child reap attempted"),
            "poll failure must attempt direct-child cleanup: {message}"
        );
        Ok(())
    }

    #[test]
    fn compiler_oracle_parser_rejects_malformed_unknown_and_duplicate_rows() -> Result<()> {
        for (source, expected) in [
            ("\tOracle::Demo\n", "missing fact family"),
            ("package\t\n", "missing fact name"),
            ("package\tOracle::Demo\textra\n", "extra fields"),
            ("type\tOracle::Demo\n", "unknown Perl oracle fact family"),
            ("package\tOracle::Demo\npackage\tOracle::Demo\n", "duplicate Perl oracle fact"),
            ("\n", "no recognized facts"),
        ] {
            let error = parse_oracle_facts(source)
                .err()
                .with_context(|| format!("oracle row {source:?} should be rejected"))?;
            let message = format!("{error:#}");
            assert!(
                message.contains(expected),
                "oracle row {source:?} should report {expected:?}, got {message:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn compiler_oracle_negative_control_tracks_removed_source_fact() -> Result<()> {
        let source_without_ordinary = FIXTURE_SOURCE.replace(ORDINARY_SUB_LINE, "");
        let rust_facts = normalize_rust_compile_effects(&lower_source(&source_without_ordinary));
        let observed = run_perl_oracle(&source_without_ordinary)?;

        assert!(
            !rust_facts.contains(&normalized("sub", "Oracle::Demo::ordinary".to_string())),
            "the Rust normalizer must change when the source fact is removed"
        );
        assert!(
            !observed.facts.contains(&normalized("sub", "Oracle::Demo::ordinary".to_string())),
            "the Perl oracle must change when the source fact is removed"
        );
        Ok(())
    }

    #[test]
    fn compiler_oracle_comparison_preserves_both_disagreement_directions() -> Result<()> {
        let observed = run_perl_oracle(FIXTURE_SOURCE)?;
        let mut rust_facts = observed.facts.clone();
        rust_facts.insert(normalized("package", "Oracle::RustOnly".to_string()));
        let mut perl_facts = observed.facts;
        perl_facts.insert(normalized("package", "Oracle::PerlOnly".to_string()));

        let receipt = compare_facts(observed.perl_version, rust_facts, perl_facts);

        assert_eq!(receipt.matched_facts, vec![normalized("sub", "Oracle::Shared".to_string())]);
        assert_eq!(
            receipt.disagreements,
            vec![
                disagreement("package", "Oracle::RustOnly".to_string(), "rust_only"),
                disagreement("package", "Oracle::PerlOnly".to_string(), "perl_only"),
            ],
            "the differential receipt must retain both one-sided failure classes"
        );
    }

    #[test]
    fn compiler_oracle_fixture_path_is_repo_independent() -> Result<()> {
        let tempdir = tempfile::tempdir().context("create compiler-oracle path test tempdir")?;
        let path = tempdir.path().join("OracleDemo.pm");
        assert!(
            is_relative_to(&path, tempdir.path()),
            "fixture path should stay in the oracle tempdir"
        );
        Ok(())
    }

    fn is_relative_to(path: &Path, root: &Path) -> bool {
        path.starts_with(root)
    }
}
