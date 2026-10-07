//! Acceptance tests for `features/merge_m6_legacy_assets.feature` (A's
//! `node_metrics_to_proto`); the scenario names are the test function names
//! verbatim. No database needed: both scenarios are pure type behaviour.

use chrono::Utc;
use protocol::NodeMetricsReport;
use storage::conversions::node_metrics_to_proto;
use storage::models::NodeMetricsRow;

const NODE_ID: &str = "m6-node";
const SEQUENCE: i64 = 42;
const TIMESTAMP_MS: i64 = 1_700_000_000_000;

fn row_with_optional_values(
    cpu: Option<f64>,
    load: Option<f64>,
    memory: Option<i64>,
) -> NodeMetricsRow {
    NodeMetricsRow {
        id: 1,
        node_id: NODE_ID.to_string(),
        sequence: SEQUENCE,
        timestamp_ms: TIMESTAMP_MS,
        monotonic_clock_ms: Some(123_456),
        cpu_usage_percent: cpu,
        load_1: load,
        load_5: load,
        load_15: load,
        memory_total_bytes: memory,
        memory_used_bytes: memory,
        swap_total_bytes: memory,
        swap_used_bytes: memory,
        uptime_seconds: Some(3_600),
        boot_time_seconds: Some(1_600_000_000),
        gpu_metrics: None,
        gpu_processes: None,
        network_metrics: None,
        disk_metrics: None,
        cpu_core_metrics: None,
        cpu_processes: None,
        created_at: Utc::now(),
    }
}

#[test]
fn a_stored_metrics_row_converts_to_the_protocol_report_without_losing_identity() {
    let row = row_with_optional_values(Some(37.5), Some(1.25), Some(8_192));

    let report: NodeMetricsReport = node_metrics_to_proto(&row);

    assert_eq!(report.node_id, NODE_ID);
    assert_eq!(report.sequence, SEQUENCE as u64);
    assert_eq!(report.timestamp_ms, TIMESTAMP_MS as u64);
    assert_eq!(report.monotonic_clock_ms, 123_456);
    assert_eq!(report.cpu_usage_percent, 37.5);
    assert_eq!(report.load_1, 1.25);
    assert_eq!(report.load_5, 1.25);
    assert_eq!(report.load_15, 1.25);
    assert_eq!(report.memory_total_bytes, 8_192);
    assert_eq!(report.memory_used_bytes, 8_192);
    assert_eq!(report.swap_total_bytes, 8_192);
    assert_eq!(report.swap_used_bytes, 8_192);
    assert_eq!(report.uptime_seconds, 3_600);
    assert_eq!(report.boot_time_seconds, 1_600_000_000);
}

#[test]
fn missing_optional_values_in_a_stored_row_convert_to_zero_instead_of_failing() {
    let mut row = row_with_optional_values(None, None, None);
    row.monotonic_clock_ms = None;

    let report: NodeMetricsReport = node_metrics_to_proto(&row);

    assert_eq!(report.node_id, NODE_ID, "identity survives the conversion");
    assert_eq!(report.sequence, SEQUENCE as u64);
    assert_eq!(report.timestamp_ms, TIMESTAMP_MS as u64);
    assert_eq!(report.cpu_usage_percent, 0.0);
    assert_eq!(report.load_1, 0.0);
    assert_eq!(report.load_5, 0.0);
    assert_eq!(report.load_15, 0.0);
    assert_eq!(report.memory_total_bytes, 0);
    assert_eq!(report.memory_used_bytes, 0);
    assert_eq!(report.swap_total_bytes, 0);
    assert_eq!(report.swap_used_bytes, 0);
    assert_eq!(
        report.monotonic_clock_ms, 0,
        "a missing monotonic clock reads as 0"
    );
}
