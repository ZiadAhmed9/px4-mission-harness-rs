use crate::report::model::Report;

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
}
