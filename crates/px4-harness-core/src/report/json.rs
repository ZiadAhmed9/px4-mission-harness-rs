use crate::report::model::{MultiVehicleReport, Report, SuiteReport};

/// Render the report as pretty-printed JSON.
pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report serialization should not fail")
}

/// Render a suite report as pretty-printed JSON.
pub fn render_suite_json(report: &SuiteReport) -> String {
    serde_json::to_string_pretty(report).expect("failed to serialize suite report")
}

/// Render a multi-vehicle report as pretty-printed JSON.
pub fn render_multi_vehicle_json(report: &MultiVehicleReport) -> String {
    serde_json::to_string_pretty(report).expect("failed to serialize multi-vehicle report")
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
            fault_stats: None,
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
                total_flight_time_secs: Some(60.0),
                path_length_m: 150.0,
                max_path_deviation_m: Some(3.5),
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

    fn sample_multi_vehicle_report() -> MultiVehicleReport {
        let vehicle1_report = Report {
            scenario_name: "Formation flight".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 0.0,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 50,
                attitude_samples: 50,
                status_samples: 5,
                final_latitude: Some(47.397742),
                final_longitude: Some(8.545594),
                final_altitude: Some(0.0),
                total_flight_time_secs: Some(45.0),
                path_length_m: 100.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "landed".to_string(),
                passed: true,
                reason: "Confirmed".to_string(),
                elapsed_secs: Some(45.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };
        let vehicle2_report = Report {
            scenario_name: "Formation flight".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 0.0,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 48,
                attitude_samples: 48,
                status_samples: 5,
                final_latitude: Some(47.398742),
                final_longitude: Some(8.545594),
                final_altitude: Some(0.0),
                total_flight_time_secs: Some(46.0),
                path_length_m: 102.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "landed".to_string(),
                passed: true,
                reason: "Confirmed".to_string(),
                elapsed_secs: Some(46.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };
        MultiVehicleReport {
            scenario_name: "Formation flight".to_string(),
            scenario_description: Some("Two-vehicle formation test".to_string()),
            vehicles: vec![
                VehicleReport {
                    system_id: 1,
                    report: vehicle1_report,
                },
                VehicleReport {
                    system_id: 2,
                    report: vehicle2_report,
                },
            ],
            inter_vehicle_assertions: vec![AssertionReport {
                name: "min_separation[30.0m]".to_string(),
                passed: true,
                reason: "All vehicle pairs maintained at least 30.0m separation".to_string(),
                elapsed_secs: None,
            }],
            all_passed: true,
        }
    }

    #[test]
    fn multi_vehicle_json_is_valid() {
        let report = sample_multi_vehicle_report();
        let json_str = render_multi_vehicle_json(&report);

        // Must be valid JSON.
        let value: serde_json::Value =
            serde_json::from_str(&json_str).expect("multi-vehicle JSON should be valid");

        assert_eq!(value["scenario_name"], "Formation flight");
        assert_eq!(value["all_passed"], true);

        // Must contain a "vehicles" array with 2 entries.
        let vehicles = value["vehicles"]
            .as_array()
            .expect("vehicles should be an array");
        assert_eq!(vehicles.len(), 2);
        assert_eq!(vehicles[0]["system_id"], 1);
        assert_eq!(vehicles[1]["system_id"], 2);

        // Must contain an "inter_vehicle_assertions" array.
        let inter = value["inter_vehicle_assertions"]
            .as_array()
            .expect("inter_vehicle_assertions should be an array");
        assert_eq!(inter.len(), 1);
        assert_eq!(inter[0]["name"], "min_separation[30.0m]");
        assert_eq!(inter[0]["passed"], true);
    }

    fn sample_suite_report() -> SuiteReport {
        let r1 = sample_report();
        let r2 = Report {
            scenario_name: "Second scenario".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 0.0,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 50,
                attitude_samples: 50,
                status_samples: 5,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "landed".to_string(),
                passed: true,
                reason: "Confirmed".to_string(),
                elapsed_secs: Some(30.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };
        SuiteReport::build("My Suite".to_string(), None, vec![r1, r2])
    }

    #[test]
    fn suite_json_is_valid() {
        let suite = sample_suite_report();
        let json_str = render_suite_json(&suite);
        let value: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(value["suite_name"], "My Suite");
        assert_eq!(value["total_scenarios"], 2);
        assert!(value["scenarios"].is_array());
        assert_eq!(value["scenarios"].as_array().unwrap().len(), 2);
    }
}
