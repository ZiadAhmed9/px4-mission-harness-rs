use crate::report::model::Report;

/// Render the report as pretty-printed JSON.
pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::*;

    #[test]
    fn json_is_valid() {
        let report = sample_report();
        let json_str = render_json(&report);
        let value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(value["scenario_name"], "Test scenario");
        assert_eq!(value["passed"], false);
        assert_eq!(value["passed_count"], 1);
        assert_eq!(value["failed_count"], 1);
    }

    fn sample_report() -> Report {
        Report {
            scenario_name: "Test scenario".to_string(),
            scenario_description: Some("A test".to_string()),
            faults: FaultSummary {
                delay_ms: 100,
                jitter_ms: 50,
                loss_rate: 0.1,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 100,
                attitude_samples: 200,
                status_samples: 10,
                final_latitude: Some(47.397742),
                final_longitude: Some(8.545594),
                final_altitude: Some(0.1),
            },
            assertions: vec![
                AssertionReport {
                    name: "waypoint_reached[0]".to_string(),
                    passed: true,
                    reason: "Reached at 12.3s".to_string(),
                    elapsed_secs: Some(12.3),
                },
                AssertionReport {
                    name: "landed".to_string(),
                    passed: false,
                    reason: "Not confirmed".to_string(),
                    elapsed_secs: None,
                },
            ],
            passed: false,
            total: 2,
            passed_count: 1,
            failed_count: 1,
        }
    }
}
