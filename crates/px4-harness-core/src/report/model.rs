use serde::Serialize;

use crate::assertion::engine::AssertionResult;
use crate::mission::controller::MissionController;
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
    pub total_flight_time_secs: Option<f64>,
    pub path_length_m: f64,
    pub max_path_deviation_m: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AssertionReport {
    pub name: String,
    pub passed: bool,
    pub reason: String,
    pub elapsed_secs: Option<f64>,
}

/// Complete report for a suite run (multiple scenarios).
#[derive(Debug, Serialize)]
pub struct SuiteReport {
    pub suite_name: String,
    pub suite_description: Option<String>,
    pub scenarios: Vec<Report>,
    pub total_scenarios: usize,
    pub passed_scenarios: usize,
    pub failed_scenarios: usize,
    pub all_passed: bool,
}

impl SuiteReport {
    pub fn build(name: String, description: Option<String>, reports: Vec<Report>) -> Self {
        let total = reports.len();
        let passed = reports.iter().filter(|r| r.passed).count();
        SuiteReport {
            suite_name: name,
            suite_description: description,
            scenarios: reports,
            total_scenarios: total,
            passed_scenarios: passed,
            failed_scenarios: total - passed,
            all_passed: passed == total,
        }
    }
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

        // Compute total_flight_time_secs: duration from first arm to first subsequent disarm.
        let total_flight_time_secs = {
            let mut first_arm_ts = None;
            let mut flight_time = None;
            for status in statuses.iter() {
                if status.armed && first_arm_ts.is_none() {
                    first_arm_ts = Some(status.timestamp);
                } else if !status.armed {
                    if let Some(arm_ts) = first_arm_ts {
                        flight_time = Some(status.timestamp.duration_since(arm_ts).as_secs_f64());
                        break;
                    }
                }
            }
            flight_time
        };

        // Compute path_length_m: sum of haversine distances between consecutive positions.
        let path_length_m = positions.windows(2).fold(0.0_f64, |acc, pair| {
            acc + MissionController::haversine_distance(
                pair[0].latitude,
                pair[0].longitude,
                pair[1].latitude,
                pair[1].longitude,
            )
        });

        // Compute max_path_deviation_m: max distance from any position to the nearest waypoint.
        let waypoints = &scenario.mission.waypoints;
        let max_path_deviation_m = if waypoints.is_empty() {
            None
        } else {
            positions
                .iter()
                .map(|pos| {
                    waypoints
                        .iter()
                        .map(|wp| {
                            MissionController::haversine_distance(
                                pos.latitude,
                                pos.longitude,
                                wp.latitude,
                                wp.longitude,
                            )
                        })
                        .fold(f64::MAX, f64::min)
                })
                .reduce(f64::max)
        };

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
                total_flight_time_secs,
                path_length_m,
                max_path_deviation_m,
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
