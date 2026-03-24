use serde::Serialize;

use crate::assertion::engine::AssertionResult;
use crate::scenario::ScenarioFile;
use crate::telemetry::store::TelemetryStore;

/// Complete report for one scenario run.
#[derive(Debug, Serialize)]
pub struct Report {
    pub scenario_name: String,
    pub scenario_description: Option<String>,
    pub faults: FaultSummary,
    pub telemetry: TelemetrySummary,
    pub assertions: Vec<AssertionReport>,
    pub passed: bool,
    pub total: usize,
    pub passed_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Serialize)]
pub struct FaultSummary {
    pub delay_ms: u64,
    pub jitter_ms: u64,
    pub loss_rate: f64,
    pub burst_loss_length: u32,
    pub duplicate_rate: f64,
    pub replay_stale_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct TelemetrySummary {
    pub position_samples: usize,
    pub attitude_samples: usize,
    pub status_samples: usize,
    pub final_latitude: Option<f64>,
    pub final_longitude: Option<f64>,
    pub final_altitude: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AssertionReport {
    pub name: String,
    pub passed: bool,
    pub reason: String,
    pub elapsed_secs: Option<f64>,
}

impl Report {
    /// Build a report from the scenario definition, telemetry store, and assertion results.
    pub fn build(
        scenario: &ScenarioFile,
        store: &TelemetryStore,
        results: &[AssertionResult],
    ) -> Self {
        let passed_count = results.iter().filter(|r| r.passed).count();
        let failed_count = results.len() - passed_count;

        let positions = store.positions.lock().unwrap();
        let attitudes = store.attitudes.lock().unwrap();
        let statuses = store.statuses.lock().unwrap();
        let last_pos = positions.last();

        Report {
            scenario_name: scenario.scenario.name.clone(),
            scenario_description: scenario.scenario.description.clone(),
            faults: FaultSummary {
                delay_ms: scenario.faults.delay_ms,
                jitter_ms: scenario.faults.jitter_ms,
                loss_rate: scenario.faults.loss_rate,
                burst_loss_length: scenario.faults.burst_loss_length,
                duplicate_rate: scenario.faults.duplicate_rate,
                replay_stale_ms: scenario.faults.replay_stale_ms,
            },
            telemetry: TelemetrySummary {
                position_samples: positions.len(),
                attitude_samples: attitudes.len(),
                status_samples: statuses.len(),
                final_latitude: last_pos.map(|p| p.latitude),
                final_longitude: last_pos.map(|p| p.longitude),
                final_altitude: last_pos.map(|p| p.relative_alt),
            },
            assertions: results
                .iter()
                .map(|r| AssertionReport {
                    name: r.name.clone(),
                    passed: r.passed,
                    reason: r.reason.clone(),
                    elapsed_secs: r.elapsed.map(|d| d.as_secs_f64()),
                })
                .collect(),
            passed: failed_count == 0,
            total: results.len(),
            passed_count,
            failed_count,
        }
    }
}
