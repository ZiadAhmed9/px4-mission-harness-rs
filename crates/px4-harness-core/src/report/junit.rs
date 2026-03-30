use crate::report::model::{MultiVehicleReport, Report, SuiteReport};

/// Render the report as JUnit XML for CI systems.
pub fn render_junit(report: &Report) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<testsuites>\n");
    xml.push_str(&format!(
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\">\n",
        escape_xml(&report.scenario_name),
        report.total,
        report.failed_count,
    ));

    for a in &report.assertions {
        let time = a.elapsed_secs.unwrap_or(0.0);
        if a.passed {
            xml.push_str(&format!(
                "    <testcase name=\"{}\" time=\"{:.1}\"/>\n",
                escape_xml(&a.name),
                time,
            ));
        } else {
            xml.push_str(&format!(
                "    <testcase name=\"{}\" time=\"{:.1}\">\n",
                escape_xml(&a.name),
                time,
            ));
            xml.push_str(&format!(
                "      <failure message=\"{}\"/>\n",
                escape_xml(&a.reason),
            ));
            xml.push_str("    </testcase>\n");
        }
    }

    xml.push_str("  </testsuite>\n");
    xml.push_str("</testsuites>\n");
    xml
}

/// Render a suite report as JUnit XML with one `<testsuite>` per scenario.
pub fn render_suite_junit(report: &SuiteReport) -> String {
    let total_tests: usize = report.scenarios.iter().map(|s| s.total).sum();
    let total_failures: usize = report.scenarios.iter().map(|s| s.failed_count).sum();

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\">\n",
        escape_xml(&report.suite_name),
        total_tests,
        total_failures,
    ));

    for scenario in &report.scenarios {
        xml.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\">\n",
            escape_xml(&scenario.scenario_name),
            scenario.total,
            scenario.failed_count,
        ));

        for a in &scenario.assertions {
            let time = a.elapsed_secs.unwrap_or(0.0);
            if a.passed {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\"/>\n",
                    escape_xml(&a.name),
                    time,
                ));
            } else {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\">\n",
                    escape_xml(&a.name),
                    time,
                ));
                xml.push_str(&format!(
                    "      <failure message=\"{}\"/>\n",
                    escape_xml(&a.reason),
                ));
                xml.push_str("    </testcase>\n");
            }
        }

        xml.push_str("  </testsuite>\n");
    }

    xml.push_str("</testsuites>\n");
    xml
}

/// Render a multi-vehicle report as JUnit XML.
///
/// Produces one `<testsuite>` per vehicle and one for inter-vehicle assertions,
/// all wrapped in a `<testsuites>` root element.
pub fn render_multi_vehicle_junit(report: &MultiVehicleReport) -> String {
    let vehicle_tests: usize = report.vehicles.iter().map(|v| v.report.total).sum();
    let vehicle_failures: usize = report.vehicles.iter().map(|v| v.report.failed_count).sum();
    let inter_tests = report.inter_vehicle_assertions.len();
    let inter_failures = report
        .inter_vehicle_assertions
        .iter()
        .filter(|a| !a.passed)
        .count();

    let total_tests = vehicle_tests + inter_tests;
    let total_failures = vehicle_failures + inter_failures;

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\">\n",
        escape_xml(&report.scenario_name),
        total_tests,
        total_failures,
    ));

    // One testsuite per vehicle
    for v in &report.vehicles {
        xml.push_str(&format!(
            "  <testsuite name=\"vehicle-{}\" tests=\"{}\" failures=\"{}\">\n",
            v.system_id, v.report.total, v.report.failed_count,
        ));
        for a in &v.report.assertions {
            let time = a.elapsed_secs.unwrap_or(0.0);
            if a.passed {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\"/>\n",
                    escape_xml(&a.name),
                    time,
                ));
            } else {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\">\n",
                    escape_xml(&a.name),
                    time,
                ));
                xml.push_str(&format!(
                    "      <failure message=\"{}\"/>\n",
                    escape_xml(&a.reason),
                ));
                xml.push_str("    </testcase>\n");
            }
        }
        xml.push_str("  </testsuite>\n");
    }

    // Testsuite for inter-vehicle assertions
    if !report.inter_vehicle_assertions.is_empty() {
        xml.push_str(&format!(
            "  <testsuite name=\"inter-vehicle\" tests=\"{}\" failures=\"{}\">\n",
            inter_tests, inter_failures,
        ));
        for a in &report.inter_vehicle_assertions {
            let time = a.elapsed_secs.unwrap_or(0.0);
            if a.passed {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\"/>\n",
                    escape_xml(&a.name),
                    time,
                ));
            } else {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.1}\">\n",
                    escape_xml(&a.name),
                    time,
                ));
                xml.push_str(&format!(
                    "      <failure message=\"{}\"/>\n",
                    escape_xml(&a.reason),
                ));
                xml.push_str("    </testcase>\n");
            }
        }
        xml.push_str("  </testsuite>\n");
    }

    xml.push_str("</testsuites>\n");
    xml
}

/// Escape special XML characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::*;

    #[test]
    fn junit_structure() {
        let report = Report {
            scenario_name: "Test".to_string(),
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
                position_samples: 0,
                attitude_samples: 0,
                status_samples: 0,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![
                AssertionReport {
                    name: "wp[0]".to_string(),
                    passed: true,
                    reason: "OK".to_string(),
                    elapsed_secs: Some(5.0),
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
        };

        let xml = render_junit(&report);
        assert!(xml.contains("<?xml"));
        assert!(xml.contains("<testsuite name=\"Test\" tests=\"2\" failures=\"1\">"));
        assert!(xml.contains("<testcase name=\"wp[0]\" time=\"5.0\"/>"));
        assert!(xml.contains("<failure message=\"Not confirmed\"/>"));
    }

    #[test]
    fn escape_special_chars() {
        assert_eq!(
            escape_xml("a < b & c > d \"e\""),
            "a &lt; b &amp; c &gt; d &quot;e&quot;"
        );
    }

    fn sample_multi_vehicle_report() -> MultiVehicleReport {
        let make_vehicle_report = |sys_id: u8| Report {
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
                position_samples: 0,
                attitude_samples: 0,
                status_samples: 0,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: format!("waypoint_reached[v{}]", sys_id),
                passed: true,
                reason: "Reached".to_string(),
                elapsed_secs: Some(10.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };

        MultiVehicleReport {
            scenario_name: "Formation flight".to_string(),
            scenario_description: None,
            vehicles: vec![
                VehicleReport {
                    system_id: 1,
                    report: make_vehicle_report(1),
                },
                VehicleReport {
                    system_id: 2,
                    report: make_vehicle_report(2),
                },
            ],
            inter_vehicle_assertions: vec![AssertionReport {
                name: "min_separation[30.0m]".to_string(),
                passed: false,
                reason: "Vehicles 1 and 2 were 5.0m apart".to_string(),
                elapsed_secs: None,
            }],
            all_passed: false,
        }
    }

    #[test]
    fn multi_vehicle_junit_structure() {
        let report = sample_multi_vehicle_report();
        let xml = render_multi_vehicle_junit(&report);

        // Must have the standard XML declaration and root testsuites element.
        assert!(xml.contains("<?xml"), "missing XML declaration");
        assert!(
            xml.contains("<testsuites"),
            "missing <testsuites root element"
        );

        // Must have per-vehicle testsuite elements named "vehicle-1" and "vehicle-2".
        assert!(
            xml.contains("name=\"vehicle-1\""),
            "missing vehicle-1 testsuite, got:\n{xml}"
        );
        assert!(
            xml.contains("name=\"vehicle-2\""),
            "missing vehicle-2 testsuite, got:\n{xml}"
        );

        // Must have an inter-vehicle testsuite element.
        assert!(
            xml.contains("name=\"inter-vehicle\""),
            "missing inter-vehicle testsuite, got:\n{xml}"
        );

        // Must contain testcase elements.
        assert!(xml.contains("<testcase"), "missing <testcase elements");

        // The inter-vehicle assertion failed, so there must be a <failure> element.
        assert!(
            xml.contains("<failure"),
            "missing <failure element for failed inter-vehicle assertion"
        );
    }

    fn sample_suite_report() -> SuiteReport {
        let r1 = Report {
            scenario_name: "Scenario One".to_string(),
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
                position_samples: 0,
                attitude_samples: 0,
                status_samples: 0,
                final_latitude: None,
                final_longitude: None,
                final_altitude: None,
                total_flight_time_secs: None,
                path_length_m: 0.0,
                max_path_deviation_m: None,
            },
            assertions: vec![AssertionReport {
                name: "wp[0]".to_string(),
                passed: true,
                reason: "OK".to_string(),
                elapsed_secs: Some(5.0),
            }],
            passed: true,
            total: 1,
            passed_count: 1,
            failed_count: 0,
        };
        let r2 = Report {
            scenario_name: "Scenario Two".to_string(),
            scenario_description: None,
            fault_stats: None,
            faults: FaultSummary {
                delay_ms: 200,
                jitter_ms: 50,
                loss_rate: 0.2,
                burst_loss_length: 0,
                duplicate_rate: 0.0,
                replay_stale_ms: 0,
            },
            telemetry: TelemetrySummary {
                position_samples: 0,
                attitude_samples: 0,
                status_samples: 0,
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
                reason: "Timed out".to_string(),
                elapsed_secs: None,
            }],
            passed: false,
            total: 1,
            passed_count: 0,
            failed_count: 1,
        };
        SuiteReport::build("Test Suite".to_string(), None, vec![r1, r2])
    }

    #[test]
    fn suite_junit_structure() {
        let suite = sample_suite_report();
        let xml = render_suite_junit(&suite);

        assert!(xml.contains("<testsuites"), "missing <testsuites");
        assert!(xml.contains("<testsuite"), "missing <testsuite");
        assert!(xml.contains("<testcase"), "missing <testcase");
        // Suite-level attributes.
        assert!(xml.contains("name=\"Test Suite\""), "missing suite name");
        // Per-scenario testsuite elements.
        assert!(
            xml.contains("name=\"Scenario One\""),
            "missing Scenario One"
        );
        assert!(
            xml.contains("name=\"Scenario Two\""),
            "missing Scenario Two"
        );
        // Failure element for the failing scenario.
        assert!(xml.contains("<failure"), "missing <failure element");
    }
}
