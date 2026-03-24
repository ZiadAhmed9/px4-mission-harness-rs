use std::time::Duration;

use crate::mission::controller::MissionController;
use crate::scenario::{Assertion, Waypoint};
use crate::telemetry::store::TelemetryStore;

/// The result of evaluating one assertion.
#[derive(Debug, Clone)]
pub struct AssertionResult {
    /// Human-readable name (e.g., "waypoint_reached[0]")
    pub name: String,
    /// Did it pass?
    pub passed: bool,
    /// Why it passed or failed
    pub reason: String,
    /// Time from mission start when the condition was met. None if failed.
    pub elapsed: Option<Duration>,
}

/// Evaluate all assertions from the scenario against collected telemetry.
pub fn evaluate_assertions(
    assertions: &[Assertion],
    waypoints: &[Waypoint],
    store: &TelemetryStore,
) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|assertion| match assertion {
            Assertion::WaypointReached {
                waypoint_index,
                timeout_secs,
            } => {
                // Check if the waypoint index is valid
                if let Some(wp) = waypoints.get(*waypoint_index) {
                    check_waypoint_reached(store, wp, *waypoint_index, *timeout_secs)
                } else {
                    AssertionResult {
                        name: format!("waypoint_reached[{}]", waypoint_index),
                        passed: false,
                        reason: format!(
                            "Waypoint index {} out of range (mission has {} waypoints)",
                            waypoint_index,
                            waypoints.len()
                        ),
                        elapsed: None,
                    }
                }
            }
            Assertion::AltitudeReached {
                altitude,
                tolerance,
                timeout_secs,
            } => check_altitude_reached(store, *altitude, *tolerance, *timeout_secs),

            Assertion::Landed { timeout_secs } => check_landed(store, *timeout_secs),
        })
        .collect()
}

/// Check if the drone reached a waypoint within the acceptance radius before the timeout.
///
/// Iterates through all position samples. For each sample within the timeout window,
/// calculates haversine distance to the waypoint. Passes if any sample is within radius.
fn check_waypoint_reached(
    store: &TelemetryStore,
    waypoint: &Waypoint,
    waypoint_index: usize,
    timeout_secs: u64,
) -> AssertionResult {
    let positions = store.positions.lock().unwrap();
    let timeout = Duration::from_secs(timeout_secs);

    let mut closest_distance = f64::MAX;

    for pos in positions.iter() {
        let elapsed = pos.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break; // past the timeout window
        }

        let distance = MissionController::haversine_distance(
            pos.latitude,
            pos.longitude,
            waypoint.latitude,
            waypoint.longitude,
        );

        if distance < closest_distance {
            closest_distance = distance;
        }

        if distance < waypoint.acceptance_radius {
            return AssertionResult {
                name: format!("waypoint_reached[{}]", waypoint_index),
                passed: true,
                reason: format!(
                    "Reached waypoint {} at {:.1}s, distance {:.1}m (radius {:.1}m)",
                    waypoint_index,
                    elapsed.as_secs_f64(),
                    distance,
                    waypoint.acceptance_radius,
                ),
                elapsed: Some(elapsed),
            };
        }
    }

    AssertionResult {
        name: format!("waypoint_reached[{}]", waypoint_index),
        passed: false,
        reason: format!(
            "Waypoint {} not reached within {}s, closest distance {:.1}m (radius {:.1}m)",
            waypoint_index, timeout_secs, closest_distance, waypoint.acceptance_radius,
        ),
        elapsed: None,
    }
}

/// Check if the drone reached a target altitude within tolerance before the timeout.
///
/// Uses relative_alt (altitude above takeoff point), not altitude above sea level.
fn check_altitude_reached(
    store: &TelemetryStore,
    target_altitude: f64,
    tolerance: f64,
    timeout_secs: u64,
) -> AssertionResult {
    let positions = store.positions.lock().unwrap();
    let timeout = Duration::from_secs(timeout_secs);

    let mut closest_diff = f64::MAX;

    for pos in positions.iter() {
        let elapsed = pos.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break;
        }

        let diff = (pos.relative_alt - target_altitude).abs();
        if diff < closest_diff {
            closest_diff = diff;
        }

        if diff <= tolerance {
            return AssertionResult {
                name: format!("altitude_reached[{:.0}m]", target_altitude),
                passed: true,
                reason: format!(
                    "Reached altitude {:.1}m at {:.1}s (target {:.1}m, tolerance {:.1}m)",
                    pos.relative_alt,
                    elapsed.as_secs_f64(),
                    target_altitude,
                    tolerance,
                ),
                elapsed: Some(elapsed),
            };
        }
    }

    AssertionResult {
        name: format!("altitude_reached[{:.0}m]", target_altitude),
        passed: false,
        reason: format!(
            "Altitude {:.1}m not reached within {}s, closest diff {:.1}m (tolerance {:.1}m)",
            target_altitude, timeout_secs, closest_diff, tolerance,
        ),
        elapsed: None,
    }
}

/// Check if the drone landed (transitioned from armed to disarmed).
///
/// We can't just check LandedState::OnGround because the drone starts on the ground.
/// Instead we look for the armed -> disarmed transition, which PX4 triggers after landing.
fn check_landed(store: &TelemetryStore, timeout_secs: u64) -> AssertionResult {
    let statuses = store.statuses.lock().unwrap();
    let timeout = Duration::from_secs(timeout_secs);

    let mut was_armed = false;

    for status in statuses.iter() {
        let elapsed = status.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break;
        }

        if status.armed {
            was_armed = true;
        }

        // Was armed, now disarmed = landed
        if was_armed && !status.armed {
            return AssertionResult {
                name: "landed".to_string(),
                passed: true,
                reason: format!(
                    "Landing confirmed at {:.1}s (disarmed after being armed)",
                    elapsed.as_secs_f64(),
                ),
                elapsed: Some(elapsed),
            };
        }
    }

    AssertionResult {
        name: "landed".to_string(),
        passed: false,
        reason: format!("Landing not confirmed within {}s", timeout_secs),
        elapsed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::store::*;

    /// Helper: create a TelemetryStore with fake position samples.
    /// Each tuple is (lat, lon, relative_alt, milliseconds_from_start).
    fn make_store_with_positions(positions: Vec<(f64, f64, f64, u64)>) -> TelemetryStore {
        let store = TelemetryStore::new();
        let start = store.mission_start;
        for (lat, lon, alt, ms) in positions {
            store.record_position(PositionSample {
                timestamp: start + Duration::from_millis(ms),
                latitude: lat,
                longitude: lon,
                altitude_msl: alt + 400.0, // fake MSL
                relative_alt: alt,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
            });
        }
        store
    }

    // --- waypoint_reached tests ---

    #[test]
    fn waypoint_reached_within_radius() {
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 10.0, 5000), // right on the waypoint at 5s
        ]);
        let wp = Waypoint {
            latitude: 47.397742,
            longitude: 8.545594,
            altitude: 10.0,
            acceptance_radius: 5.0,
        };
        let result = check_waypoint_reached(&store, &wp, 0, 60);
        assert!(result.passed, "{}", result.reason);
    }

    #[test]
    fn waypoint_not_reached_too_far() {
        let store = make_store_with_positions(vec![
            (47.400000, 8.550000, 10.0, 5000), // far away
        ]);
        let wp = Waypoint {
            latitude: 47.397742,
            longitude: 8.545594,
            altitude: 10.0,
            acceptance_radius: 5.0,
        };
        let result = check_waypoint_reached(&store, &wp, 0, 60);
        assert!(!result.passed, "should fail: {}", result.reason);
    }

    #[test]
    fn waypoint_not_reached_timeout() {
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 10.0, 70000), // on waypoint but after 70s (timeout 60s)
        ]);
        let wp = Waypoint {
            latitude: 47.397742,
            longitude: 8.545594,
            altitude: 10.0,
            acceptance_radius: 5.0,
        };
        let result = check_waypoint_reached(&store, &wp, 0, 60);
        assert!(
            !result.passed,
            "should fail due to timeout: {}",
            result.reason
        );
    }

    // --- altitude_reached tests ---

    #[test]
    fn altitude_reached_within_tolerance() {
        let store = make_store_with_positions(vec![
            (47.0, 8.0, 9.8, 5000), // 9.8m, target 10m, tolerance 0.5m
        ]);
        let result = check_altitude_reached(&store, 10.0, 0.5, 60);
        assert!(result.passed, "{}", result.reason);
    }

    #[test]
    fn altitude_not_reached() {
        let store = make_store_with_positions(vec![
            (47.0, 8.0, 5.0, 5000), // 5m, target 10m, tolerance 0.5m
        ]);
        let result = check_altitude_reached(&store, 10.0, 0.5, 60);
        assert!(!result.passed, "should fail: {}", result.reason);
    }

    // --- landed tests ---

    #[test]
    fn landed_after_armed() {
        let store = TelemetryStore::new();
        let start = store.mission_start;
        // Armed at 2s
        store.record_status(VehicleStatus {
            timestamp: start + Duration::from_secs(2),
            armed: true,
            flight_mode: 0,
            system_status: 0,
        });
        // Disarmed (landed) at 30s
        store.record_status(VehicleStatus {
            timestamp: start + Duration::from_secs(30),
            armed: false,
            flight_mode: 0,
            system_status: 0,
        });
        let result = check_landed(&store, 120);
        assert!(result.passed, "{}", result.reason);
    }

    #[test]
    fn not_landed_still_armed() {
        let store = TelemetryStore::new();
        let start = store.mission_start;
        // Armed at 2s, never disarmed
        store.record_status(VehicleStatus {
            timestamp: start + Duration::from_secs(2),
            armed: true,
            flight_mode: 0,
            system_status: 0,
        });
        let result = check_landed(&store, 120);
        assert!(!result.passed, "should fail: {}", result.reason);
    }
}
