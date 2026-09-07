//! Nearest-rank percentile arithmetic owned by workspace SLO reporting.
//!
//! This is measurement math, not Perl syntax: it belongs to the SLO reporter
//! that publishes P50/P95/P99 latency, and it is deliberately private to
//! [`crate::slo`] so no product or parser surface can grow a dependency on it
//! (#7595). The parser previously re-exported an identical helper as
//! `perl_parser_core::percentile`; that public path is retired.
//!
//! # Contract
//!
//! For a slice sorted ascending and a percentile `pct`:
//!
//! - `pct` is clamped to `0..=100`;
//! - `rank = ceil(pct * n / 100)` for `n` samples;
//! - the result is the sample at `rank - 1`, with `rank == 0` reading the
//!   first sample;
//! - an empty sample set yields `0`.
//!
//! The rank is computed with integer ceiling division rather than floating
//! point so the boundary between two adjacent ranks is exact for every sample
//! count. The sibling bench scorecards under `crates/perl-*/benches/support/`
//! use the same `ceil` formula on their own private samples; the identity to
//! preserve across all of them is this documented rank definition, not a
//! shared implementation.

/// Compute the nearest-rank percentile of a slice sorted in ascending order.
///
/// Returns `0` for an empty sample set. `pct` above 100 is clamped to 100.
#[must_use]
pub(crate) fn nearest_rank_percentile(sorted_values: &[u64], pct: u64) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }

    // `pct` is clamped to 0..=100 before the conversion, so the fallback is
    // unreachable; it exists so the helper stays total without a panic path.
    let pct_clamped = usize::try_from(pct.min(100)).unwrap_or(100);
    let len = sorted_values.len();
    let rank = len.saturating_mul(pct_clamped).div_ceil(100).min(len);

    sorted_values[rank.saturating_sub(1)]
}

#[cfg(test)]
mod tests {
    use super::nearest_rank_percentile;

    #[test]
    fn empty_samples_return_zero() {
        assert_eq!(nearest_rank_percentile(&[], 0), 0);
        assert_eq!(nearest_rank_percentile(&[], 50), 0);
        assert_eq!(nearest_rank_percentile(&[], 95), 0);
        assert_eq!(nearest_rank_percentile(&[], 100), 0);
    }

    #[test]
    fn reported_percentiles_match_expected_samples() {
        let sorted = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        assert_eq!(nearest_rank_percentile(&sorted, 0), 1);
        assert_eq!(nearest_rank_percentile(&sorted, 50), 5);
        assert_eq!(nearest_rank_percentile(&sorted, 95), 10);
        assert_eq!(nearest_rank_percentile(&sorted, 99), 10);
        assert_eq!(nearest_rank_percentile(&sorted, 100), 10);
    }

    #[test]
    fn single_sample_answers_every_percentile() {
        let sorted = [42];

        assert_eq!(nearest_rank_percentile(&sorted, 0), 42);
        assert_eq!(nearest_rank_percentile(&sorted, 50), 42);
        assert_eq!(nearest_rank_percentile(&sorted, 95), 42);
        assert_eq!(nearest_rank_percentile(&sorted, 100), 42);
        assert_eq!(nearest_rank_percentile(&sorted, 150), 42);
    }

    #[test]
    fn percentiles_above_one_hundred_clamp_to_the_maximum() {
        let sorted = [10, 20, 30];

        assert_eq!(nearest_rank_percentile(&sorted, 100), 30);
        assert_eq!(nearest_rank_percentile(&sorted, 200), 30);
        assert_eq!(nearest_rank_percentile(&sorted, 1000), 30);
        assert_eq!(nearest_rank_percentile(&sorted, u64::MAX), 30);
    }

    #[test]
    fn fractional_ranks_round_up() {
        let sorted = [5, 10, 15, 20];

        // rank = ceil(0.26 * 4) = 2 -> second sample, not the first.
        assert_eq!(nearest_rank_percentile(&sorted, 26), 10);
        // rank = ceil(0.25 * 4) = 1 -> exactly the first sample.
        assert_eq!(nearest_rank_percentile(&sorted, 25), 5);
    }

    #[test]
    fn duplicate_samples_keep_their_rank() {
        let sorted = [3, 3, 3, 7, 7, 9];

        assert_eq!(nearest_rank_percentile(&sorted, 50), 3);
        assert_eq!(nearest_rank_percentile(&sorted, 67), 7);
    }

    #[test]
    fn rank_boundaries_select_adjacent_samples() {
        let sorted = [10, 20, 30, 40, 50];

        assert_eq!(nearest_rank_percentile(&sorted, 1), 10);
        assert_eq!(nearest_rank_percentile(&sorted, 20), 10);
        assert_eq!(nearest_rank_percentile(&sorted, 21), 20);
        assert_eq!(nearest_rank_percentile(&sorted, 80), 40);
        assert_eq!(nearest_rank_percentile(&sorted, 81), 50);
        assert_eq!(nearest_rank_percentile(&sorted, 100), 50);
    }

    #[test]
    fn p95_uses_the_ceiling_rank_for_small_sample_counts() {
        // The floor formula ((n * 95) / 100) returns the maximum sample for
        // every n <= 20; the nearest-rank definition must not.
        let sorted: Vec<u64> = (1..=20).collect();

        assert_eq!(nearest_rank_percentile(&sorted, 95), 19);
    }

    #[test]
    fn exact_ranks_are_not_inflated_by_floating_point_error() {
        // Negative control for the retired `(pct as f64 / 100.0) * n` rank:
        // 28% of 25 samples is exactly rank 7, but the f64 product is
        // 7.000000000000001, so the floating formula rounds up to rank 8 and
        // reports the wrong sample. Integer ceiling division cannot.
        let sorted: Vec<u64> = (1..=25).collect();
        assert_eq!(nearest_rank_percentile(&sorted, 28), 7);

        let sorted: Vec<u64> = (1..=50).collect();
        assert_eq!(nearest_rank_percentile(&sorted, 14), 7);
    }

    #[test]
    fn reported_slo_percentiles_are_unchanged_by_the_exact_rank() {
        // P50/P95/P99 are the only percentiles the SLO reporter asks for, and
        // the exact rank agrees with the retired floating rank for every
        // sample count the tracker can hold.
        for n in 1..=512usize {
            let sorted: Vec<u64> = (0..n as u64).collect();
            for pct in [50, 95, 99] {
                // The retired helper's rank, verbatim.
                let rank = ((pct as f64 / 100.0) * n as f64).ceil() as usize;
                let expected = sorted[rank.min(n).saturating_sub(1)];
                assert_eq!(
                    nearest_rank_percentile(&sorted, pct as u64),
                    expected,
                    "n={n} pct={pct}"
                );
            }
        }
    }

    #[test]
    fn large_sample_counts_do_not_overflow_the_rank() {
        let sorted: Vec<u64> = (0..10_000).collect();

        assert_eq!(nearest_rank_percentile(&sorted, 50), 4_999);
        assert_eq!(nearest_rank_percentile(&sorted, 95), 9_499);
        assert_eq!(nearest_rank_percentile(&sorted, 99), 9_899);
        assert_eq!(nearest_rank_percentile(&sorted, 100), 9_999);
    }
}
