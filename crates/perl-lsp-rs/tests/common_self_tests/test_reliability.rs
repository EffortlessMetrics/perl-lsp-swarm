#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]

use crate::common::test_reliability::*;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_validation() -> Result<(), String> {
        let env = TestEnvironment::validate()?;
        assert!(env.thread_count > 0);
        eprintln!("Test environment: {}", env.summary());
        Ok(())
    }

    #[test]
    fn test_timeout_multiplier_reasonable() {
        let env = TestEnvironment::validate().unwrap();
        let multiplier = env.timeout_multiplier();
        assert!(multiplier >= 1.0);
        assert!(multiplier <= 10.0);
    }

    #[test]
    fn test_resource_monitor_tracks_time() {
        let monitor = ResourceMonitor::start("test_operation");
        std::thread::sleep(Duration::from_millis(10));
        monitor.complete();
    }

    #[test]
    fn test_graceful_degradation_succeeds_eventually() {
        let mut degradation = GracefulDegradation::new(3);
        let mut attempt_count = 0;

        let result = degradation.attempt(|| {
            attempt_count += 1;
            if attempt_count < 2 { Err("simulated failure") } else { Ok(42) }
        });

        assert_eq!(result, Ok(42));
        assert_eq!(attempt_count, 2);
    }

    #[test]
    fn test_truncate_long_strings() {
        let long_str = "a".repeat(100);
        let truncated = truncate(&long_str, 10);
        assert_eq!(truncated.len(), 10);
        assert!(truncated.ends_with("..."));
    }
}
