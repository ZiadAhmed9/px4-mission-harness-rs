use crate::report::model::{MultiVehicleReport, Report, SuiteReport};

/// Render the report as a Markdown document.
pub fn render_markdown(report: &Report) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# {}\n\n", report.scenario_name));
    if let Some(desc) = &report.scenario_description {
        md.push_str(&format!("{}\n\n", desc));
    }

    // Overall result
    let status = if report.passed { "PASSED" } else { "FAILED" };
    md.push_str(&format!(
        "**Result: {}** — {} passed, {} failed, {} total\n\n",
        status, report.passed_count, report.failed_count, report.total
    ));

    // Fault profile
    md.push_str("## Fault Profile\n\n");
    md.push_str("| Parameter | Value |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!("| Delay | {}ms |\n", report.faults.delay_ms));
    md.push_str(&format!("| Jitter | {}ms |\n", report.faults.jitter_ms));
    md.push_str(&format!(
        "| Loss rate | {:.0}% |\n",
        report.faults.loss_rate * 100.0
    ));
    md.push_str(&format!(
        "| Burst loss | {} |\n",
        report.faults.burst_loss_length
    ));
    md.push_str(&format!(
        "| Duplicate rate | {:.0}% |\n",
        report.faults.duplicate_rate * 100.0
    ));
    md.push_str(&format!(
        "| Replay stale | {}ms |\n\n",
        report.faults.replay_stale_ms
    ));

    // Assertions
    md.push_str("## Assertions\n\n");
    md.push_str("| Status | Name | Details | Time |\n");
    md.push_str("|---|---|---|---|\n");
    for a in &report.assertions {
        let icon = if a.passed { "PASS" } else { "FAIL" };
        let time = a
            .elapsed_secs
            .map(|s| format!("{:.1}s", s))
            .unwrap_or_else(|| "-".to_string());
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            icon, a.name, a.reason, time
        ));
    }
    md.push('\n');

    // Telemetry summary
    md.push_str("## Telemetry\n\n");
    md.push_str(&format!(
        "- Position samples: {}\n",
        report.telemetry.position_samples
    ));
    md.push_str(&format!(
        "- Attitude samples: {}\n",
        report.telemetry.attitude_samples
    ));
    md.push_str(&format!(
        "- Status samples: {}\n",
        report.telemetry.status_samples
    ));
    if let Some(lat) = report.telemetry.final_latitude {
        md.push_str(&format!(
            "- Final position: ({:.6}, {:.6}) alt={:.1}m\n",
            lat,
            report.telemetry.final_longitude.unwrap_or(0.0),
            report.telemetry.final_altitude.unwrap_or(0.0),
        ));
    }

    md
}

/// Render a suite report as a Markdown document.
pub fn render_suite_markdown(report: &SuiteReport) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Suite Report: {}\n\n", report.suite_name));
    if let Some(desc) = &report.suite_description {
        md.push_str(&format!("{}\n\n", desc));
    }

    // Summary
    md.push_str("## Summary\n\n");
    let overall = if report.all_passed {
        "PASSED"
    } else {
        "FAILED"
    };
    md.push_str(&format!(
        "**Overall: {}** — {}/{} scenarios passed\n\n",
        overall, report.passed_scenarios, report.total_scenarios
    ));

    // Scenario results table
    md.push_str("## Scenario Results\n\n");
    md.push_str("| Scenario | Result | Passed | Failed | Loss Rate | Delay |\n");
    md.push_str("|----------|--------|--------|--------|-----------|-------|\n");
    for s in &report.scenarios {
        let result = if s.passed { "PASS" } else { "FAIL" };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {:.0}% | {}ms |\n",
            s.scenario_name,
            result,
            s.passed_count,
            s.failed_count,
            s.faults.loss_rate * 100.0,
            s.faults.delay_ms,
        ));
    }
    md.push('\n');

    // Per-scenario details
    md.push_str("## Per-Scenario Details\n\n");
    for s in &report.scenarios {
        md.push_str(&format!("### {}\n\n", s.scenario_name));
        if let Some(desc) = &s.scenario_description {
            md.push_str(&format!("{}\n\n", desc));
        }
        md.push_str("| Status | Name | Details | Time |\n");
        md.push_str("|--------|------|---------|------|\n");
        for a in &s.assertions {
            let icon = if a.passed { "PASS" } else { "FAIL" };
            let time = a
                .elapsed_secs
                .map(|secs| format!("{:.1}s", secs))
                .unwrap_or_else(|| "-".to_string());
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                icon, a.name, a.reason, time
            ));
        }
        md.push('\n');
    }

    md
}

/// Render a multi-vehicle report as a Markdown document.
pub fn render_multi_vehicle_markdown(report: &MultiVehicleReport) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Multi-Vehicle: {}\n\n", report.scenario_name));
    if let Some(desc) = &report.scenario_description {
        md.push_str(&format!("{}\n\n", desc));
    }

    // Summary
    let overall = if report.all_passed {
        "PASSED"
    } else {
        "FAILED"
    };
    md.push_str(&format!(
        "**Result: {}** — {} vehicles\n\n",
        overall,
        report.vehicles.len()
    ));

    // Per-vehicle summary table
    md.push_str("## Vehicle Summary\n\n");
    md.push_str("| System ID | Result | Passed | Failed | Total |\n");
    md.push_str("|-----------|--------|--------|--------|-------|\n");
    for v in &report.vehicles {
        let result = if v.report.passed { "PASS" } else { "FAIL" };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            v.system_id, result, v.report.passed_count, v.report.failed_count, v.report.total,
        ));
    }
    md.push('\n');

    // Inter-vehicle assertions table
    if !report.inter_vehicle_assertions.is_empty() {
        md.push_str("## Inter-Vehicle Assertions\n\n");
        md.push_str("| Status | Name | Details | Time |\n");
        md.push_str("|--------|------|---------|------|\n");
        for a in &report.inter_vehicle_assertions {
            let icon = if a.passed { "PASS" } else { "FAIL" };
            let time = a
                .elapsed_secs
                .map(|s| format!("{:.1}s", s))
                .unwrap_or_else(|| "-".to_string());
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                icon, a.name, a.reason, time
            ));
        }
        md.push('\n');
    }

    // Per-vehicle details
    md.push_str("## Per-Vehicle Details\n\n");
    for v in &report.vehicles {
        md.push_str(&format!(
            "### Vehicle {} (system_id={})\n\n",
            v.system_id, v.system_id
        ));
        md.push_str("| Status | Name | Details | Time |\n");
        md.push_str("|--------|------|---------|------|\n");
        for a in &v.report.assertions {
            let icon = if a.passed { "PASS" } else { "FAIL" };
            let time = a
                .elapsed_secs
                .map(|secs| format!("{:.1}s", secs))
                .unwrap_or_else(|| "-".to_string());
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                icon, a.name, a.reason, time
            ));
        }
        md.push('\n');
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::*;

    #[test]
    fn markdown_contains_key_sections() {
        let report = Report {
            scenario_name: "Test scenario".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 0.1,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 50,
                attitude_samples: 100,
                status_samples: 5,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "waypoint_reached[0]".to_string(),
                passed: true,
                reason: "Reached".to_string(),
                elapsed_secs: Some(10.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };

        let md = render_markdown(&report);
        assert!(md.contains("# Test scenario"));
        assert!(md.contains("PASSED"));
        assert!(md.contains("## Fault Profile"));
        assert!(md.contains("## Assertions"));
        assert!(md.contains("waypoint_reached[0]"));
        assert!(md.contains("## Telemetry"));
    }

    fn sample_multi_vehicle_report() -> MultiVehicleReport {
        let make_vehicle_report = |name: &str, sys_id: u8| Report {
            scenario_name: name.to_string(),
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
                position_samples: 40,
                attitude_samples: 40,
                status_samples: 4,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: Some(30.0),
                path_length_m: 80.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: format!("landed[v{}]", sys_id),
                passed: true,
                reason: "Confirmed".to_string(),
                elapsed_secs: Some(30.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };

        MultiVehicleReport {
            scenario_name: "Formation mission".to_string(),
            scenario_description: Some("Multi-vehicle formation test".to_string()),
            vehicles: vec![
                VehicleReport {
                    system_id: 1,
                    report: make_vehicle_report("Formation mission", 1),
                },
                VehicleReport {
                    system_id: 2,
                    report: make_vehicle_report("Formation mission", 2),
                },
            ],
            inter_vehicle_assertions: vec![AssertionReport {
                name: "min_separation[50.0m]".to_string(),
                passed: true,
                reason: "All vehicle pairs maintained at least 50.0m separation".to_string(),
                elapsed_secs: None,
            }],
            all_passed: true,
        }
    }

    #[test]
    fn multi_vehicle_markdown_contains_key_sections() {
        let report = sample_multi_vehicle_report();
        let md = render_multi_vehicle_markdown(&report);

        // Must have a "Multi-Vehicle" header with the scenario name.
        assert!(
            md.contains("Multi-Vehicle"),
            "markdown should contain 'Multi-Vehicle', got:\n{md}"
        );
        assert!(
            md.contains("Formation mission"),
            "markdown should contain the scenario name, got:\n{md}"
        );

        // Must mention both vehicle system IDs.
        assert!(
            md.contains('1') && md.contains('2'),
            "markdown should contain vehicle system IDs 1 and 2"
        );

        // Must contain an inter-vehicle section.
        assert!(
            md.contains("Inter-Vehicle"),
            "markdown should contain an inter-vehicle section, got:\n{md}"
        );

        // Must show the min_separation assertion result.
        assert!(
            md.contains("min_separation"),
            "markdown should contain min_separation assertion, got:\n{md}"
        );
    }

    fn sample_suite_report() -> SuiteReport {
        let r1 = Report {
            scenario_name: "Alpha scenario".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 100,
                jitter_ms: 0,
                loss_rate: 0.1,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 80,
                attitude_samples: 80,
                status_samples: 8,
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
                elapsed_secs: Some(45.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };
        let r2 = Report {
            scenario_name: "Beta scenario".to_string(),
            scenario_description: Some("Heavy loss test".to_string()),
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 0.5,
                burst_loss_length: 3,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 20,
                attitude_samples: 20,
                status_samples: 2,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "landed".to_string(),
                passed: false,
                reason: "Not confirmed".to_string(),
                elapsed_secs: None,
            }],
            passed: false,
            total: 1,
            passed_count: 0,
            failed_count: 1,
        };
        SuiteReport::build(
            "Regression Suite".to_string(),
            Some("Fault tolerance tests".to_string()),
            vec![r1, r2],
        )
    }

    #[test]
    fn suite_markdown_contains_key_sections() {
        let suite = sample_suite_report();
        let md = render_suite_markdown(&suite);

        assert!(md.contains("Suite Report:"), "missing suite header");
        assert!(md.contains("## Summary"), "missing Summary section");
        assert!(
            md.contains("## Scenario Results"),
            "missing Scenario Results section"
        );
        assert!(md.contains("Alpha scenario"), "missing first scenario name");
        assert!(md.contains("Beta scenario"), "missing second scenario name");
        // Overall result should show FAILED because one scenario failed.
        assert!(md.contains("FAILED"));
    }
}
