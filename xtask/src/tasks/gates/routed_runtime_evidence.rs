//! Executed-result validation for the selected routed runtime (#17482).
use std::io::BufRead;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RuntimeCounts {
    pub passed: u32,
    pub ignored: u32,
}

/// Stream this attempt's freshly truncated log, pairing each libtest preamble
/// with its successful terminal summary. Empty binaries are legitimate within
/// a nonempty portfolio, but ignored-only and compile-only runs cannot pass.
pub(super) fn validate(reader: impl BufRead) -> Result<RuntimeCounts, String> {
    let mut pending = None;
    let mut counts = RuntimeCounts { passed: 0, ignored: 0 };
    for line in reader.lines() {
        let line = line.map_err(|error| format!("runtime log could not be read: {error}"))?;
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("running ") {
            let fields: Vec<_> = rest.split_whitespace().collect();
            if fields.len() == 2 && matches!(fields[1], "test" | "tests") {
                let selected = fields[0].parse::<u32>().map_err(|_| "invalid libtest count")?;
                if pending.replace(selected).is_some() {
                    return Err("libtest population lacks its terminal summary".into());
                }
            }
        } else if let Some(rest) = line.strip_prefix("test result: ") {
            let selected = pending.take().ok_or("libtest summary lacks its invocation preamble")?;
            let rest = rest.strip_prefix("ok. ").ok_or("libtest population did not pass")?;
            let fields: Vec<_> = rest.split(';').collect();
            if fields.len() < 5 {
                return Err("incomplete libtest summary".into());
            }
            let count = |index: usize, label: &str| -> Result<u32, String> {
                fields[index]
                    .trim()
                    .strip_suffix(label)
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .ok_or_else(|| format!("invalid libtest {label} count"))
            };
            let passed = count(0, "passed")?;
            let failed = count(1, "failed")?;
            let ignored = count(2, "ignored")?;
            let measured = count(3, "measured")?;
            let _filtered = count(4, "filtered out")?;
            let completed = passed.checked_add(ignored).ok_or("libtest count overflow")?;
            if failed != 0 || measured != 0 || completed != selected {
                return Err("libtest summary contradicts its selected runtime population".into());
            }
            counts.passed = counts.passed.checked_add(passed).ok_or("libtest count overflow")?;
            counts.ignored = counts.ignored.checked_add(ignored).ok_or("libtest count overflow")?;
        }
    }
    if pending.is_some() {
        return Err("libtest population lacks its terminal summary".into());
    }
    if counts.passed == 0 {
        return Err("selected routed runtime executed no passing tests".into());
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(log: &str) -> Result<RuntimeCounts, String> {
        validate(log.as_bytes())
    }
    const PASS: &str = "running 1 test\ntest example ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    #[test]
    fn passing_populations_sum_and_empty_binaries_are_allowed() {
        let log = format!(
            "{PASS}running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n{PASS}"
        );
        assert_eq!(check(&log), Ok(RuntimeCounts { passed: 2, ignored: 0 }));
    }
    #[test]
    fn compile_only_zero_and_ignored_only_are_not_runtime_success() {
        for log in [
            "Finished test profile\n",
            "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n",
            "running 1 test\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;\n",
        ] {
            assert!(check(log).is_err(), "{log}");
        }
    }
    #[test]
    fn misleading_foreign_incomplete_and_failing_summaries_are_refused() {
        for log in [
            "note: test result: ok. 1 passed\n",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n",
            "running 1 test\n",
            "running 2 tests\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n",
            "running 1 test\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;\n",
        ] {
            assert!(check(log).is_err(), "{log}");
        }
        assert!(check(&format!("{PASS}running 1 test\n")).is_err());
    }
    #[test]
    fn read_failure_is_not_success() {
        assert!(validate(std::io::BufReader::new(&b"\xff\n"[..])).is_err());
    }
}
