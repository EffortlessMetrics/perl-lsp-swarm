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

    sorted_values[nearest_rank(sorted_values.len(), pct).saturating_sub(1)]
}

/// The 1-based nearest rank for a window of `len` samples at percentile `pct`.
///
/// Returns `0` only for an empty window or `pct == 0`; otherwise the result is
/// in `1..=len`.
fn nearest_rank(len: usize, pct: u64) -> usize {
    let pct_clamped = u128::from(pct.min(100));
    // Widen before multiplying. `len * pct` overflows a `usize` once the window
    // passes `usize::MAX / 100` — roughly 45 million samples on a 32-bit target
    // — and a saturated product would clamp the rank far below the true one,
    // silently understating every reported percentile instead of failing.
    //
    // Both fallbacks below are unreachable: a `usize` always fits a `u128`, and
    // `pct` is clamped to 100 first, so the rank never exceeds `len`. They exist
    // so the helper stays total without a panic path.
    let width = u128::try_from(len).unwrap_or(u128::MAX);
    let rank = (width * pct_clamped).div_ceil(100);

    usize::try_from(rank).unwrap_or(len).min(len)
}

#[cfg(test)]
mod tests {
    use super::{nearest_rank, nearest_rank_percentile};

    #[test]
    fn ranks_stay_exact_for_windows_whose_product_overflows_a_usize() {
        // `len * pct` in `usize` overflows once `len` passes `usize::MAX / 100`
        // — about 45 million on a 32-bit target, and reachable on 64-bit too. A
        // saturating product answers a rank far below the true one, so the
        // reported percentile silently drifts toward the median. The rank is a
        // function of the window width alone, so these cases cost no allocation.

        // 32-bit overflow width: 200_000_000 * 95 is 1.9e10, past u32 range.
        assert_eq!(nearest_rank(200_000_000, 95), 190_000_000);

        // 64-bit overflow width: usize::MAX * 100 overflows on every target.
        assert_eq!(nearest_rank(usize::MAX, 100), usize::MAX);
        assert_eq!(nearest_rank(usize::MAX, 50), usize::MAX / 2 + 1);
        assert_eq!(nearest_rank(usize::MAX, 0), 0);
    }

    #[test]
    fn ranks_are_one_based_and_bounded_by_the_window() {
        assert_eq!(nearest_rank(0, 95), 0);
        assert_eq!(nearest_rank(10, 0), 0);
        assert_eq!(nearest_rank(10, 1), 1);
        assert_eq!(nearest_rank(10, 100), 10);
        assert_eq!(nearest_rank(10, u64::MAX), 10);
    }

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
        // P50/P95/P99 are the only percentiles the SLO reporter asks for. The
        // exact rank agrees with the retired floating rank for every sample
        // count from 1 through 1_200 — past `SloConfig::sample_window_size`'s
        // 1_000 default — plus a spread of larger windows, because that field
        // is public and a caller may configure a wider one.
        let counts = (1..=1_200usize).chain([2_000, 5_000, 10_000, 65_536, 100_000]);

        for n in counts {
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
