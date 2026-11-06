use crate::common::timeout_scaler::*;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_thread_count_returns_at_least_one() {
        assert!(effective_thread_count() >= 1);
    }

    #[test]
    fn test_adaptive_timeout_returns_reasonable_duration() {
        let timeout = get_adaptive_timeout();
        assert!(timeout >= Duration::from_secs(5));
        assert!(timeout <= Duration::from_secs(30)); // Even with CI multiplier
    }

    #[test]
    fn test_scaled_timeout_applies_factor() {
        let base = Duration::from_secs_f64(11.25);
        let doubled = scale_duration(base, 2.0);
        let halved = scale_duration(base, 0.5);

        // Allow for floating-point imprecision
        assert!((doubled.as_secs_f64() - base.as_secs_f64() * 2.0).abs() < 0.01);
        assert!((halved.as_secs_f64() - base.as_secs_f64() * 0.5).abs() < 0.01);
    }

    #[test]
    fn test_short_timeout_is_shorter_than_adaptive() {
        let short = get_short_timeout();
        let adaptive = get_adaptive_timeout();
        assert!(short < adaptive);
    }

    #[test]
    fn test_init_timeout_is_longer_than_adaptive() {
        let init = get_init_timeout();
        let adaptive = get_adaptive_timeout();
        assert!(init > adaptive);
    }

    #[test]
    fn test_timeout_profiles_have_correct_ordering() {
        let quick = TimeoutProfile::Quick.timeout();
        let perf = TimeoutProfile::Performance.timeout();
        let standard = TimeoutProfile::Standard.timeout();
        let init = TimeoutProfile::Initialization.timeout();
        let stress = TimeoutProfile::Stress.timeout();

        // Quick should be shortest, stress should be longest
        assert!(quick <= perf);
        assert!(standard <= init);
        assert!(init <= stress);
    }

    #[test]
    fn test_retry_config_exponential_backoff() {
        let config = RetryConfig::default();

        let t0 = config.timeout_for_attempt(0);
        let t1 = config.timeout_for_attempt(1);
        let t2 = config.timeout_for_attempt(2);

        // Each attempt should be longer than the previous
        assert!(t1 > t0);
        assert!(t2 > t1);

        // But capped at max_timeout
        let t10 = config.timeout_for_attempt(10);
        assert!(t10 <= config.max_timeout);
    }
}
