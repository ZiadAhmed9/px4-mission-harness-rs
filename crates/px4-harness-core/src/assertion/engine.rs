use std::collections::HashMap;
use std::sync::Arc;
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

            Assertion::SegmentTiming {
                from_waypoint,
                to_waypoint,
                max_duration_secs,
            } => check_segment_timing(
                store,
                waypoints,
                *from_waypoint,
                *to_waypoint,
                *max_duration_secs,
            ),

            Assertion::Geofence {
                max_altitude,
                max_distance_m,
                timeout_secs,
            } => check_geofence(
                store,
                waypoints,
                *max_altitude,
                *max_distance_m,
                *timeout_secs,
            ),

            Assertion::MaxTilt {
                max_degrees,
                timeout_secs,
            } => check_max_tilt(store, *max_degrees, *timeout_secs),

            Assertion::MaxGroundSpeed {
                max_speed_ms,
                timeout_secs,
            } => check_max_ground_speed(store, *max_speed_ms, *timeout_secs),

            // MinSeparation is an inter-vehicle assertion — it cannot be evaluated against
            // a single vehicle's telemetry store. Skip it here; it is handled by
            // evaluate_multi_vehicle_assertions.
            Assertion::MinSeparation { .. } => AssertionResult {
                name: "min_separation".to_string(),
                passed: false,
                reason: "MinSeparation is an inter-vehicle assertion and cannot be evaluated \
                         against a single telemetry store"
                    .to_string(),
                elapsed: None,
            },
        })
        .collect()
}

/// Evaluate inter-vehicle assertions that compare telemetry across vehicles.
/// `stores` maps system_id to its TelemetryStore.
pub fn evaluate_multi_vehicle_assertions(
    assertions: &[Assertion],
    stores: &HashMap<u8, Arc<TelemetryStore>>,
) -> Vec<AssertionResult> {
    assertions
        .iter()
        .filter_map(|assertion| match assertion {
            Assertion::MinSeparation {
                min_distance_m,
                timeout_secs,
            } => Some(check_min_separation(stores, *min_distance_m, *timeout_secs)),
            _ => None,
        })
        .collect()
}

/// Check that all vehicle pairs maintained at least `min_distance_m` separation at all times.
///
/// For each position sample from vehicle A, finds the nearest-timestamp sample from vehicle B
/// and computes the haversine distance. Fails if any pair is closer than `min_distance_m`.
fn check_min_separation(
    stores: &HashMap<u8, Arc<TelemetryStore>>,
    min_distance_m: f64,
    timeout_secs: u64,
) -> AssertionResult {
    let name = format!("min_separation[{:.1}m]", min_distance_m);
    let timeout = Duration::from_secs(timeout_secs);

    // Collect (system_id, positions) pairs for all vehicles that have data.
    let vehicle_ids: Vec<u8> = stores.keys().copied().collect();

    if vehicle_ids.len() < 2 {
        return AssertionResult {
            name,
            passed: true,
            reason: "fewer than two vehicles with telemetry — separation check skipped".to_string(),
            elapsed: None,
        };
    }

    // Check each unique pair.
    for i in 0..vehicle_ids.len() {
        for j in (i + 1)..vehicle_ids.len() {
            let id_a = vehicle_ids[i];
            let id_b = vehicle_ids[j];

            let store_a = &stores[&id_a];
            let store_b = &stores[&id_b];

            let positions_a = store_a.positions.lock().unwrap();
            let positions_b = store_b.positions.lock().unwrap();

            if positions_a.is_empty() || positions_b.is_empty() {
                continue;
            }

            let mission_start = store_a.mission_start;

            for pos_a in positions_a.iter() {
                let elapsed = pos_a.timestamp.duration_since(mission_start);
                if elapsed > timeout {
                    break;
                }

                // Find the position sample from vehicle B with the closest timestamp.
                let nearest_b = positions_b.iter().min_by_key(|pos_b| {
                    if pos_b.timestamp > pos_a.timestamp {
                        pos_b.timestamp.duration_since(pos_a.timestamp)
                    } else {
                        pos_a.timestamp.duration_since(pos_b.timestamp)
                    }
                });

                if let Some(pos_b) = nearest_b {
                    let distance = MissionController::haversine_distance(
                        pos_a.latitude,
                        pos_a.longitude,
                        pos_b.latitude,
                        pos_b.longitude,
                    );

                    if distance < min_distance_m {
                        return AssertionResult {
                            name,
                            passed: false,
                            reason: format!(
                                "Vehicles {} and {} were {:.1}m apart at {:.1}s \
                                 (min required: {:.1}m)",
                                id_a,
                                id_b,
                                distance,
                                elapsed.as_secs_f64(),
                                min_distance_m,
                            ),
                            elapsed: None,
                        };
                    }
                }
            }
        }
    }

    AssertionResult {
        name,
        passed: true,
        reason: format!(
            "All vehicle pairs maintained at least {:.1}m separation",
            min_distance_m
        ),
        elapsed: None,
    }
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

/// Check that the segment between two waypoints was completed within max_duration_secs.
///
/// Finds when from_waypoint was first reached, then checks whether to_waypoint was
/// reached within max_duration_secs after that.
fn check_segment_timing(
    store: &TelemetryStore,
    waypoints: &[Waypoint],
    from_waypoint: usize,
    to_waypoint: usize,
    max_duration_secs: u64,
) -> AssertionResult {
    let name = format!("segment_timing[{}->{}]", from_waypoint, to_waypoint);

    let from_wp = match waypoints.get(from_waypoint) {
        Some(wp) => wp,
        None => {
            return AssertionResult {
                name,
                passed: false,
                reason: format!(
                    "from_waypoint index {} out of range (mission has {} waypoints)",
                    from_waypoint,
                    waypoints.len()
                ),
                elapsed: None,
            };
        }
    };

    let to_wp = match waypoints.get(to_waypoint) {
        Some(wp) => wp,
        None => {
            return AssertionResult {
                name,
                passed: false,
                reason: format!(
                    "to_waypoint index {} out of range (mission has {} waypoints)",
                    to_waypoint,
                    waypoints.len()
                ),
                elapsed: None,
            };
        }
    };

    let positions = store.positions.lock().unwrap();

    // Find the first time from_waypoint was reached.
    let from_reached_at = positions.iter().find_map(|pos| {
        let dist = MissionController::haversine_distance(
            pos.latitude,
            pos.longitude,
            from_wp.latitude,
            from_wp.longitude,
        );
        if dist < from_wp.acceptance_radius {
            Some(pos.timestamp)
        } else {
            None
        }
    });

    let from_instant = match from_reached_at {
        Some(t) => t,
        None => {
            return AssertionResult {
                name,
                passed: false,
                reason: format!("prerequisite waypoint {} not reached", from_waypoint),
                elapsed: None,
            };
        }
    };

    let max_duration = Duration::from_secs(max_duration_secs);

    // Find the first time to_waypoint was reached after from_waypoint.
    let to_reached = positions
        .iter()
        .filter(|pos| pos.timestamp >= from_instant)
        .find_map(|pos| {
            let dist = MissionController::haversine_distance(
                pos.latitude,
                pos.longitude,
                to_wp.latitude,
                to_wp.longitude,
            );
            if dist < to_wp.acceptance_radius {
                Some(pos.timestamp)
            } else {
                None
            }
        });

    match to_reached {
        Some(to_instant) => {
            let segment_duration = to_instant.duration_since(from_instant);
            if segment_duration <= max_duration {
                let elapsed = to_instant.duration_since(store.mission_start);
                AssertionResult {
                    name,
                    passed: true,
                    reason: format!(
                        "Segment {}→{} completed in {:.1}s (max {}s)",
                        from_waypoint,
                        to_waypoint,
                        segment_duration.as_secs_f64(),
                        max_duration_secs,
                    ),
                    elapsed: Some(elapsed),
                }
            } else {
                AssertionResult {
                    name,
                    passed: false,
                    reason: format!(
                        "Segment {}→{} took {:.1}s, exceeded max {}s",
                        from_waypoint,
                        to_waypoint,
                        segment_duration.as_secs_f64(),
                        max_duration_secs,
                    ),
                    elapsed: None,
                }
            }
        }
        None => AssertionResult {
            name,
            passed: false,
            reason: format!(
                "Waypoint {} not reached within {}s after waypoint {}",
                to_waypoint, max_duration_secs, from_waypoint,
            ),
            elapsed: None,
        },
    }
}

/// Check that the drone stayed within geofence bounds: max altitude and max distance
/// from any waypoint.
fn check_geofence(
    store: &TelemetryStore,
    waypoints: &[Waypoint],
    max_altitude: f64,
    max_distance_m: f64,
    timeout_secs: u64,
) -> AssertionResult {
    let name = "geofence".to_string();
    let positions = store.positions.lock().unwrap();
    let timeout = Duration::from_secs(timeout_secs);

    for pos in positions.iter() {
        let elapsed = pos.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break;
        }

        if pos.relative_alt > max_altitude {
            return AssertionResult {
                name,
                passed: false,
                reason: format!(
                    "Altitude {:.1}m exceeded max {:.1}m at {:.1}s",
                    pos.relative_alt,
                    max_altitude,
                    elapsed.as_secs_f64(),
                ),
                elapsed: None,
            };
        }

        if !waypoints.is_empty() {
            let min_dist = waypoints
                .iter()
                .map(|wp| {
                    MissionController::haversine_distance(
                        pos.latitude,
                        pos.longitude,
                        wp.latitude,
                        wp.longitude,
                    )
                })
                .fold(f64::MAX, f64::min);

            if min_dist > max_distance_m {
                return AssertionResult {
                    name,
                    passed: false,
                    reason: format!(
                        "Position at {:.1}s is {:.1}m from nearest waypoint, exceeded max {:.1}m",
                        elapsed.as_secs_f64(),
                        min_dist,
                        max_distance_m,
                    ),
                    elapsed: None,
                };
            }
        }
    }

    AssertionResult {
        name,
        passed: true,
        reason: format!(
            "All positions within geofence (alt<={:.1}m, dist<={:.1}m)",
            max_altitude, max_distance_m,
        ),
        elapsed: None,
    }
}

/// Check that the drone never exceeded max_degrees of tilt.
///
/// Tilt is computed as acos(cos(roll) * cos(pitch)) converted to degrees.
fn check_max_tilt(store: &TelemetryStore, max_degrees: f64, timeout_secs: u64) -> AssertionResult {
    let name = format!("max_tilt[{:.1}deg]", max_degrees);
    let attitudes = store.attitudes.lock().unwrap();

    if attitudes.is_empty() {
        return AssertionResult {
            name,
            passed: false,
            reason: "no attitude data".to_string(),
            elapsed: None,
        };
    }

    let timeout = Duration::from_secs(timeout_secs);

    for att in attitudes.iter() {
        let elapsed = att.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break;
        }

        let tilt_deg = f32::acos(f32::cos(att.roll) * f32::cos(att.pitch)).to_degrees() as f64;

        if tilt_deg > max_degrees {
            return AssertionResult {
                name,
                passed: false,
                reason: format!(
                    "Tilt {:.2}deg exceeded max {:.1}deg at {:.1}s",
                    tilt_deg,
                    max_degrees,
                    elapsed.as_secs_f64(),
                ),
                elapsed: None,
            };
        }
    }

    AssertionResult {
        name,
        passed: true,
        reason: format!("Tilt stayed within {:.1}deg", max_degrees),
        elapsed: None,
    }
}

/// Check that the drone's ground speed never exceeded max_speed_ms.
///
/// Ground speed is sqrt(vx^2 + vy^2); vz is excluded intentionally.
fn check_max_ground_speed(
    store: &TelemetryStore,
    max_speed_ms: f64,
    timeout_secs: u64,
) -> AssertionResult {
    let name = format!("max_ground_speed[{:.1}m/s]", max_speed_ms);
    let positions = store.positions.lock().unwrap();
    let timeout = Duration::from_secs(timeout_secs);

    for pos in positions.iter() {
        let elapsed = pos.timestamp.duration_since(store.mission_start);
        if elapsed > timeout {
            break;
        }

        let speed = ((pos.vx * pos.vx + pos.vy * pos.vy).sqrt()) as f64;

        if speed > max_speed_ms {
            return AssertionResult {
                name,
                passed: false,
                reason: format!(
                    "Ground speed {:.2}m/s exceeded max {:.1}m/s at {:.1}s",
                    speed,
                    max_speed_ms,
                    elapsed.as_secs_f64(),
                ),
                elapsed: None,
            };
        }
    }

    AssertionResult {
        name,
        passed: true,
        reason: format!("Ground speed stayed within {:.1}m/s", max_speed_ms),
        elapsed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::store::*;
    use std::sync::Arc;

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

    /// Helper: add attitude samples to an existing store.
    /// Each tuple is (roll_rad, pitch_rad, yaw_rad, milliseconds_from_start).
    fn add_attitudes_to_store(store: &TelemetryStore, attitudes: Vec<(f32, f32, f32, u64)>) {
        let start = store.mission_start;
        for (roll, pitch, yaw, ms) in attitudes {
            store.record_attitude(AttitudeSample {
                timestamp: start + Duration::from_millis(ms),
                roll,
                pitch,
                yaw,
            });
        }
    }

    /// Helper: create a TelemetryStore with position samples that include velocity.
    /// Each tuple is (lat, lon, relative_alt, vx, vy, vz, milliseconds_from_start).
    fn make_store_with_velocity(
        samples: Vec<(f64, f64, f64, f32, f32, f32, u64)>,
    ) -> TelemetryStore {
        let store = TelemetryStore::new();
        let start = store.mission_start;
        for (lat, lon, alt, vx, vy, vz, ms) in samples {
            store.record_position(PositionSample {
                timestamp: start + Duration::from_millis(ms),
                latitude: lat,
                longitude: lon,
                altitude_msl: alt + 400.0,
                relative_alt: alt,
                vx,
                vy,
                vz,
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

    // --- segment_timing tests ---

    fn two_waypoints() -> Vec<Waypoint> {
        vec![
            Waypoint {
                latitude: 47.397742,
                longitude: 8.545594,
                altitude: 10.0,
                acceptance_radius: 5.0,
            },
            Waypoint {
                latitude: 47.397742,
                longitude: 8.546500,
                altitude: 10.0,
                acceptance_radius: 5.0,
            },
        ]
    }

    fn one_waypoint() -> Vec<Waypoint> {
        vec![Waypoint {
            latitude: 47.397742,
            longitude: 8.545594,
            altitude: 10.0,
            acceptance_radius: 5.0,
        }]
    }

    /// Two waypoints; from_waypoint reached at 5s, to_waypoint reached at 12s.
    /// Segment takes 7s which is under the 10s limit — PASS.
    #[test]
    fn segment_timing_pass() {
        // WP0 at (47.397742, 8.545594), WP1 slightly east
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 10.0, 5_000),  // at WP0, 5s
            (47.397742, 8.546500, 10.0, 12_000), // at WP1, 12s
        ]);
        let result = check_segment_timing(&store, &two_waypoints(), 0, 1, 10);
        assert!(result.passed, "expected pass: {}", result.reason);
    }

    /// Segment takes 20s which exceeds the 10s limit — FAIL.
    #[test]
    fn segment_timing_fail_too_slow() {
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 10.0, 5_000),  // at WP0, 5s
            (47.397742, 8.546500, 10.0, 25_000), // at WP1, 25s (20s after WP0)
        ]);
        let result = check_segment_timing(&store, &two_waypoints(), 0, 1, 10);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    /// from_waypoint (WP0) is never reached — FAIL with "not reached" in the reason.
    #[test]
    fn segment_timing_fail_prerequisite_not_reached() {
        // Far from both waypoints
        let store = make_store_with_positions(vec![(47.400000, 8.550000, 10.0, 5_000)]);
        let result = check_segment_timing(&store, &two_waypoints(), 0, 1, 10);
        assert!(!result.passed, "expected fail: {}", result.reason);
        assert!(
            result.reason.contains("not reached"),
            "reason should mention 'not reached', got: {}",
            result.reason
        );
    }

    /// from_waypoint reached but to_waypoint never reached — FAIL.
    #[test]
    fn segment_timing_fail_to_waypoint_not_reached() {
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 10.0, 5_000), // at WP0, never near WP1
        ]);
        let result = check_segment_timing(&store, &two_waypoints(), 0, 1, 10);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    // --- geofence tests ---

    /// All position samples within max_altitude=15m and max_distance_m=50m of waypoints — PASS.
    #[test]
    fn geofence_pass_within_bounds() {
        let store = make_store_with_positions(vec![(47.397742, 8.545594, 10.0, 5_000)]);
        let result = check_geofence(&store, &one_waypoint(), 15.0, 50.0, 120);
        assert!(result.passed, "expected pass: {}", result.reason);
    }

    /// One sample exceeds max_altitude=15m — FAIL.
    #[test]
    fn geofence_fail_altitude_exceeded() {
        let store = make_store_with_positions(vec![
            (47.397742, 8.545594, 16.0, 5_000), // relative_alt 16 > max 15
        ]);
        let result = check_geofence(&store, &one_waypoint(), 15.0, 50.0, 120);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    /// One sample is ~1.1 km from the waypoint, exceeding max_distance_m=50m — FAIL.
    #[test]
    fn geofence_fail_distance_exceeded() {
        // ~0.01 deg lat offset ≈ 1.1 km
        let store = make_store_with_positions(vec![(47.407742, 8.545594, 10.0, 5_000)]);
        let result = check_geofence(&store, &one_waypoint(), 15.0, 50.0, 120);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    /// No position samples — geofence loops over zero items and finds no violation — PASS.
    /// The function only fails on a detected violation; absence of data is not a breach.
    #[test]
    fn geofence_empty_telemetry_passes() {
        let store = TelemetryStore::new();
        let result = check_geofence(&store, &one_waypoint(), 15.0, 50.0, 120);
        assert!(result.passed, "empty telemetry: {}", result.reason);
    }

    // --- max_tilt tests ---

    /// Small tilt (~8 degrees) within max_degrees=15 — PASS.
    #[test]
    fn max_tilt_pass() {
        let store = TelemetryStore::new();
        // roll=0.1 rad (~5.7 deg), pitch=0.1 rad (~5.7 deg)
        // combined tilt = acos(cos(0.1)*cos(0.1)) ≈ 8.1 degrees
        add_attitudes_to_store(&store, vec![(0.1, 0.1, 0.0, 1_000)]);
        let result = check_max_tilt(&store, 15.0, 60);
        assert!(result.passed, "expected pass: {}", result.reason);
    }

    /// Large tilt (~24 degrees) exceeds max_degrees=15 — FAIL.
    #[test]
    fn max_tilt_fail_exceeded() {
        let store = TelemetryStore::new();
        // roll=0.3 rad (~17.2 deg), pitch=0.3 rad (~17.2 deg)
        // combined tilt = acos(cos(0.3)*cos(0.3)) ≈ 24.3 degrees
        add_attitudes_to_store(&store, vec![(0.3, 0.3, 0.0, 1_000)]);
        let result = check_max_tilt(&store, 15.0, 60);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    /// No attitude data at all — FAIL with reason containing "no attitude".
    #[test]
    fn max_tilt_fail_no_attitude_data() {
        let store = TelemetryStore::new();
        let result = check_max_tilt(&store, 15.0, 60);
        assert!(!result.passed, "expected fail: {}", result.reason);
        assert!(
            result.reason.contains("no attitude"),
            "reason should mention 'no attitude', got: {}",
            result.reason
        );
    }

    /// Verify the combined-axis formula: roll=0.175 rad (~10 deg), pitch=0.175 rad (~10 deg).
    /// Combined tilt ≈ 14.1 degrees. Should FAIL with max_degrees=13, PASS with max_degrees=15.
    #[test]
    fn max_tilt_combined_axes() {
        let store = TelemetryStore::new();
        // 10 degrees in radians ≈ 0.17453 rad
        add_attitudes_to_store(&store, vec![(0.175, 0.175, 0.0, 1_000)]);

        let result_fail = check_max_tilt(&store, 13.0, 60);
        assert!(
            !result_fail.passed,
            "combined tilt ~14.1 deg should exceed 13 deg limit: {}",
            result_fail.reason
        );

        let result_pass = check_max_tilt(&store, 15.0, 60);
        assert!(
            result_pass.passed,
            "combined tilt ~14.1 deg should be within 15 deg limit: {}",
            result_pass.reason
        );
    }

    // --- max_ground_speed tests ---

    /// vx=3.0, vy=4.0 → speed=5.0, max=10.0 — PASS.
    #[test]
    fn max_ground_speed_pass() {
        let store =
            make_store_with_velocity(vec![(47.397742, 8.545594, 10.0, 3.0, 4.0, 0.0, 1_000)]);
        let result = check_max_ground_speed(&store, 10.0, 60);
        assert!(result.passed, "expected pass: {}", result.reason);
    }

    /// vx=8.0, vy=6.0 → speed=10.0, max=9.0 — FAIL.
    #[test]
    fn max_ground_speed_fail_exceeded() {
        let store =
            make_store_with_velocity(vec![(47.397742, 8.545594, 10.0, 8.0, 6.0, 0.0, 1_000)]);
        let result = check_max_ground_speed(&store, 9.0, 60);
        assert!(!result.passed, "expected fail: {}", result.reason);
    }

    /// vz=20.0 but vx=vy=0.0 → ground speed=0.0, max=10.0 — PASS (vz is excluded).
    #[test]
    fn max_ground_speed_excludes_vz() {
        let store =
            make_store_with_velocity(vec![(47.397742, 8.545594, 10.0, 0.0, 0.0, 20.0, 1_000)]);
        let result = check_max_ground_speed(&store, 10.0, 60);
        assert!(
            result.passed,
            "vz should not count toward ground speed: {}",
            result.reason
        );
    }

    // --- multi-vehicle / min_separation tests ---

    /// Build a HashMap<u8, Arc<TelemetryStore>> from a list of (system_id, positions) pairs.
    /// Positions format: (lat, lon, relative_alt, milliseconds_from_start).
    fn make_multi_vehicle_stores(
        vehicles: Vec<(u8, Vec<(f64, f64, f64, u64)>)>,
    ) -> HashMap<u8, Arc<TelemetryStore>> {
        let mut stores = HashMap::new();
        for (sys_id, positions) in vehicles {
            let store = Arc::new(make_store_with_positions(positions));
            stores.insert(sys_id, store);
        }
        stores
    }

    /// Vehicle 1: lat=47.397742, lon=8.545594 (Zurich area)
    /// Vehicle 2: lat=47.398742, lon=8.545594 (~111m north)
    /// Both sampled at 5s. min_distance_m=30.0 — PASS.
    #[test]
    fn min_separation_pass() {
        let stores = make_multi_vehicle_stores(vec![
            (1, vec![(47.397742, 8.545594, 10.0, 5_000)]),
            (2, vec![(47.398742, 8.545594, 10.0, 5_000)]),
        ]);
        let assertions = vec![crate::scenario::Assertion::MinSeparation {
            min_distance_m: 30.0,
            timeout_secs: 60,
        }];
        let results = evaluate_multi_vehicle_assertions(&assertions, &stores);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "expected pass: {}", results[0].reason);
    }

    /// Vehicle 1: lat=47.397742, lon=8.545594
    /// Vehicle 2: lat=47.397742, lon=8.545604 (very close, ~1m east)
    /// min_distance_m=30.0 — FAIL.
    #[test]
    fn min_separation_fail_too_close() {
        let stores = make_multi_vehicle_stores(vec![
            (1, vec![(47.397742, 8.545594, 10.0, 5_000)]),
            (2, vec![(47.397742, 8.545604, 10.0, 5_000)]),
        ]);
        let assertions = vec![crate::scenario::Assertion::MinSeparation {
            min_distance_m: 30.0,
            timeout_secs: 60,
        }];
        let results = evaluate_multi_vehicle_assertions(&assertions, &stores);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed, "expected fail: {}", results[0].reason);
    }

    /// Only one vehicle has position data; separation check is skipped — PASS.
    #[test]
    fn min_separation_one_vehicle_no_data() {
        // Vehicle 1 has positions; vehicle 2 has none.
        let store1 = Arc::new(make_store_with_positions(vec![(
            47.397742, 8.545594, 10.0, 5_000,
        )]));
        let store2 = Arc::new(TelemetryStore::new()); // empty
        let mut stores = HashMap::new();
        stores.insert(1u8, store1);
        stores.insert(2u8, store2);

        let assertions = vec![crate::scenario::Assertion::MinSeparation {
            min_distance_m: 30.0,
            timeout_secs: 60,
        }];
        let results = evaluate_multi_vehicle_assertions(&assertions, &stores);
        assert_eq!(results.len(), 1);
        // The implementation skips pairs where either store is empty, so the assertion passes.
        assert!(
            results[0].passed,
            "single vehicle with data should not trigger a separation violation: {}",
            results[0].reason
        );
    }

    /// Three vehicles: V1 and V2 are far apart (~111m), but V1 and V3 are very close (~1m).
    /// min_distance_m=30.0 — FAIL because the V1/V3 pair violates the constraint.
    #[test]
    fn min_separation_three_vehicles() {
        let stores = make_multi_vehicle_stores(vec![
            (1, vec![(47.397742, 8.545594, 10.0, 5_000)]),
            // V2 is ~111m north of V1 — compliant
            (2, vec![(47.398742, 8.545594, 10.0, 5_000)]),
            // V3 is ~1m east of V1 — violation
            (3, vec![(47.397742, 8.545604, 10.0, 5_000)]),
        ]);
        let assertions = vec![crate::scenario::Assertion::MinSeparation {
            min_distance_m: 30.0,
            timeout_secs: 60,
        }];
        let results = evaluate_multi_vehicle_assertions(&assertions, &stores);
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].passed,
            "one pair too close should cause failure: {}",
            results[0].reason
        );
    }
}
