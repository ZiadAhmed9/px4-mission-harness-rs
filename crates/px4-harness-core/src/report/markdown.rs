use crate::report::model::Report;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::*;

    #[test]
    fn markdown_contains_key_sections() {
        let report = Report {
            scenario_name: "Test scenario".to_string(),
            scenario_description: None,
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
}
