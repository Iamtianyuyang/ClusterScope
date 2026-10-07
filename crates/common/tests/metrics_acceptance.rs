//! Acceptance tests for `features/merge_m6_legacy_assets.feature` (A's
//! `MetricsAggregation`); the scenario names are the test function names
//! verbatim.

use common::metrics::MetricsAggregation;

#[test]
fn metric_aggregation_reports_average_extremes_and_sample_count() {
    let values = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
    let agg = MetricsAggregation::new("m6-gpu_util".to_string(), &values, 1_000, 2_000);

    assert_eq!(agg.avg, 55.0);
    assert_eq!(agg.max, 100.0);
    assert_eq!(agg.min, 10.0);
    assert_eq!(agg.count, 10);
    assert!(
        agg.p95 >= agg.min && agg.p95 <= agg.max,
        "p95 must stay inside the sample range, got {}",
        agg.p95
    );
    assert_eq!(agg.metric_name, "m6-gpu_util");
    assert_eq!(agg.start_time_ms, 1_000);
    assert_eq!(agg.end_time_ms, 2_000);
}

#[test]
fn metric_aggregation_of_an_empty_sample_is_zeroed_with_count_zero() {
    let agg = MetricsAggregation::new("m6-empty".to_string(), &[], 1_000, 2_000);

    assert_eq!(agg.avg, 0.0);
    assert_eq!(agg.max, 0.0);
    assert_eq!(agg.min, 0.0);
    assert_eq!(agg.p95, 0.0);
    assert_eq!(agg.count, 0);
    assert_eq!(agg.start_time_ms, 1_000, "the start timestamp is kept");
    assert_eq!(agg.end_time_ms, 2_000, "the end timestamp is kept");
}
