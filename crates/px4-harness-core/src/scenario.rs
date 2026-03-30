//Rust structs that mirror the TOML format. Serde will automatically convert between TOML and these types.

use crate::error::HarnessError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

//Struct of the entire scenario file, which includes metadata, mission details, fault profiles, and assertions.
#[derive(Debug, Deserialize)]
pub struct ScenarioFile {
    pub scenario: ScenarioMeta,
    pub mission: Mission,
    pub faults: FaultProfile,
    pub assertions: Vec<Assertion>,
    /// Optional time-based fault phases. When empty, only `faults` is used.
    #[serde(default)]
    pub fault_phases: Vec<FaultPhase>,
    /// Optional per-vehicle configurations for multi-vehicle scenarios.
    /// When empty, the scenario is treated as single-vehicle.
    #[serde(default)]
    pub vehicles: Vec<VehicleConfig>,
}

//ScenarioMeta struct contains the name and an optional description of the scenario. The description is optional and will default to None if not provided in the TOML file.
#[derive(Debug, Deserialize)]
pub struct ScenarioMeta {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Mission {
    pub takeoff_altitude: f64,
    pub waypoints: Vec<Waypoint>,
}

#[derive(Debug, Deserialize)]
pub struct Waypoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    #[serde(default = "default_acceptance_radius")]
    pub acceptance_radius: f64,
}

fn default_acceptance_radius() -> f64 {
    5.0
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct FaultProfile {
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub jitter_ms: u64,
    #[serde(default)]
    pub loss_rate: f64,
    #[serde(default)]
    pub burst_loss_length: u32,
    #[serde(default)]
    pub duplicate_rate: f64,
    #[serde(default)]
    pub replay_stale_ms: u64,
}

/// A time-bounded fault phase that overrides the default fault profile during its active window.
#[derive(Debug, Deserialize, Clone)]
pub struct FaultPhase {
    /// Seconds after mission start when this phase becomes active. Must be >= 0.
    pub after_secs: f64,
    /// How long this phase is active (seconds). Must be > 0.
    pub duration_secs: f64,
    /// Fault parameters active during this phase.
    #[serde(flatten)]
    pub profile: FaultProfile,
}

/// Per-vehicle configuration for multi-vehicle scenarios.
#[derive(Debug, Deserialize, Clone)]
pub struct VehicleConfig {
    /// MAVLink system ID for this vehicle. Must not be 0 (broadcast) or 255 (GCS reserved).
    pub system_id: u8,
    /// UDP port where PX4 sends MAVLink output for this vehicle.
    pub px4_port: u16,
    /// UDP port the proxy listens on for the client side of this vehicle.
    pub proxy_port: u16,
    /// Per-vehicle fault profile. If absent, falls back to the scenario-level `faults`.
    #[serde(default)]
    pub faults: Option<FaultProfile>,
    /// Per-vehicle fault phases. If absent, falls back to the scenario-level `fault_phases`.
    #[serde(default)]
    pub fault_phases: Option<Vec<FaultPhase>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Assertion {
    #[serde(rename = "waypoint_reached")]
    WaypointReached {
        waypoint_index: usize,
        timeout_secs: u64,
    },

    #[serde(rename = "landed")]
    Landed { timeout_secs: u64 },

    #[serde(rename = "altitude_reached")]
    AltitudeReached {
        altitude: f64,
        tolerance: f64,
        timeout_secs: u64,
    },

    #[serde(rename = "segment_timing")]
    SegmentTiming {
        from_waypoint: usize,
        to_waypoint: usize,
        max_duration_secs: u64,
    },

    #[serde(rename = "geofence")]
    Geofence {
        max_altitude: f64,
        max_distance_m: f64,
        timeout_secs: u64,
    },

    #[serde(rename = "max_tilt")]
    MaxTilt { max_degrees: f64, timeout_secs: u64 },

    #[serde(rename = "max_ground_speed")]
    MaxGroundSpeed {
        max_speed_ms: f64,
        timeout_secs: u64,
    },

    /// Inter-vehicle assertion: minimum separation distance between any two vehicles.
    #[serde(rename = "min_separation")]
    MinSeparation {
        /// Minimum distance in meters between any two vehicles at all times.
        min_distance_m: f64,
        timeout_secs: u64,
    },
}

//Implementation of the ScenarioFile struct
// which includes a method to load a scenario from a TOML file.
//The method reads the file content and attempts to parse it into a ScenarioFile struct,
//returning any errors encountered during reading or parsing as HarnessError variants.
impl ScenarioFile {
    pub fn load(path: &Path) -> Result<Self, HarnessError> {
        // Read the content of the scenario file into a string.
        // If there's an error during reading
        // it maps the error to a HarnessError::ScenarioFileRead variant
        // including the file path and the original error source.
        let content =
            std::fs::read_to_string(path).map_err(|source| HarnessError::ScenarioFileRead {
                path: path.display().to_string(),
                source,
            })?;

        // Parse the TOML string into a ScenarioFile struct.
        // The `?` operator uses the `#[from]` attribute on ScenarioParse
        // to automatically convert toml::de::Error into HarnessError::ScenarioParse.
        let scenario: ScenarioFile = toml::from_str(&content)?;
        scenario.validate()?;
        Ok(scenario)
    }

    /// Validate scenario values are within acceptable ranges and constraints.
    fn validate(&self) -> Result<(), HarnessError> {
        if self.faults.loss_rate < 0.0 || self.faults.loss_rate > 1.0 {
            return Err(HarnessError::ScenarioValidation {
                reason: format!(
                    "loss_rate must be between 0.0 and 1.0, got {}",
                    self.faults.loss_rate
                ),
            });
        }
        if self.faults.duplicate_rate < 0.0 || self.faults.duplicate_rate > 1.0 {
            return Err(HarnessError::ScenarioValidation {
                reason: format!(
                    "duplicate_rate must be between 0.0 and 1.0, got {}",
                    self.faults.duplicate_rate
                ),
            });
        }
        for (i, phase) in self.fault_phases.iter().enumerate() {
            if phase.after_secs < 0.0 {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "fault_phases[{i}] after_secs must be >= 0.0, got {}",
                        phase.after_secs
                    ),
                });
            }
            if phase.duration_secs <= 0.0 {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "fault_phases[{i}] duration_secs must be > 0.0, got {}",
                        phase.duration_secs
                    ),
                });
            }
            if phase.profile.loss_rate < 0.0 || phase.profile.loss_rate > 1.0 {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "fault_phases[{i}] loss_rate must be between 0.0 and 1.0, got {}",
                        phase.profile.loss_rate
                    ),
                });
            }
            if phase.profile.duplicate_rate < 0.0 || phase.profile.duplicate_rate > 1.0 {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "fault_phases[{i}] duplicate_rate must be between 0.0 and 1.0, got {}",
                        phase.profile.duplicate_rate
                    ),
                });
            }
        }
        if self.mission.waypoints.is_empty() {
            return Err(HarnessError::ScenarioValidation {
                reason: "mission must have at least one waypoint".to_string(),
            });
        }
        if self.mission.takeoff_altitude <= 0.0 {
            return Err(HarnessError::ScenarioValidation {
                reason: format!(
                    "takeoff_altitude must be positive, got {}",
                    self.mission.takeoff_altitude
                ),
            });
        }
        for (i, wp) in self.mission.waypoints.iter().enumerate() {
            if wp.acceptance_radius <= 0.0 {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "waypoint[{i}] acceptance_radius must be > 0.0, got {}",
                        wp.acceptance_radius
                    ),
                });
            }
            if !(-90.0..=90.0).contains(&wp.latitude) {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "waypoint[{i}] latitude must be between -90.0 and 90.0, got {}",
                        wp.latitude
                    ),
                });
            }
            if !(-180.0..=180.0).contains(&wp.longitude) {
                return Err(HarnessError::ScenarioValidation {
                    reason: format!(
                        "waypoint[{i}] longitude must be between -180.0 and 180.0, got {}",
                        wp.longitude
                    ),
                });
            }
        }

        // Validate multi-vehicle config if present
        if !self.vehicles.is_empty() {
            let mut system_ids = HashSet::new();
            let mut px4_ports = HashSet::new();
            let mut proxy_ports = HashSet::new();

            for (i, vehicle) in self.vehicles.iter().enumerate() {
                if vehicle.system_id == 0 {
                    return Err(HarnessError::ScenarioValidation {
                        reason: format!(
                            "vehicles[{i}] system_id cannot be 0 (MAVLink broadcast address)"
                        ),
                    });
                }
                if vehicle.system_id == 255 {
                    return Err(HarnessError::ScenarioValidation {
                        reason: format!("vehicles[{i}] system_id cannot be 255 (reserved for GCS)"),
                    });
                }
                if !system_ids.insert(vehicle.system_id) {
                    return Err(HarnessError::ScenarioValidation {
                        reason: format!(
                            "vehicles[{i}] has duplicate system_id {}",
                            vehicle.system_id
                        ),
                    });
                }
                if !px4_ports.insert(vehicle.px4_port) {
                    return Err(HarnessError::ScenarioValidation {
                        reason: format!(
                            "vehicles[{i}] has duplicate px4_port {}",
                            vehicle.px4_port
                        ),
                    });
                }
                if !proxy_ports.insert(vehicle.proxy_port) {
                    return Err(HarnessError::ScenarioValidation {
                        reason: format!(
                            "vehicles[{i}] has duplicate proxy_port {}",
                            vehicle.proxy_port
                        ),
                    });
                }
            }
        }

        Ok(())
    }

    /// Returns true if this is a multi-vehicle scenario.
    pub fn is_multi_vehicle(&self) -> bool {
        !self.vehicles.is_empty()
    }
}

//Unit tests for the ScenarioFile struct and its loading functionality.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_scenario() {
        let toml_str = r#"
            [scenario]
            name = "Test mission"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.397742
            longitude = 8.545594
            altitude = 10.0

            [faults]
            loss_rate = 0.1

            [[assertions]]
            type = "waypoint_reached"
            waypoint_index = 0
            timeout_secs = 60

            [[assertions]]
            type = "landed"
            timeout_secs = 120
        "#;

        let scenario: ScenarioFile = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.scenario.name, "Test mission");
        assert_eq!(scenario.mission.waypoints.len(), 1);
        assert_eq!(scenario.faults.loss_rate, 0.1);
        assert_eq!(scenario.assertions.len(), 2);
    }

    #[test]
    fn reject_invalid_loss_rate() {
        let toml_str = r#"
            [scenario]
            name = "Bad scenario"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]
            loss_rate = 1.5

            [[assertions]]
            type = "landed"
            timeout_secs = 60
        "#;

        let scenario: ScenarioFile = toml::from_str(toml_str).unwrap();
        let result = scenario.validate();
        assert!(result.is_err());
    }

    // --- Helper: minimal valid TOML with overrideable fault fields ---

    fn valid_toml_with_faults(faults_block: &str) -> String {
        format!(
            r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]
            {faults_block}

            [[assertions]]
            type = "landed"
            timeout_secs = 60
            "#
        )
    }

    fn valid_toml_with_waypoint(wp_block: &str) -> String {
        format!(
            r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            {wp_block}

            [faults]

            [[assertions]]
            type = "landed"
            timeout_secs = 60
            "#
        )
    }

    #[test]
    fn loss_rate_zero_is_valid() {
        let toml_str = valid_toml_with_faults("loss_rate = 0.0");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn loss_rate_one_is_valid() {
        let toml_str = valid_toml_with_faults("loss_rate = 1.0");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn reject_negative_loss_rate() {
        let toml_str = valid_toml_with_faults("loss_rate = -0.1");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn duplicate_rate_zero_is_valid() {
        let toml_str = valid_toml_with_faults("duplicate_rate = 0.0");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn duplicate_rate_one_is_valid() {
        let toml_str = valid_toml_with_faults("duplicate_rate = 1.0");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn reject_invalid_duplicate_rate() {
        let toml_str = valid_toml_with_faults("duplicate_rate = 1.5");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_negative_duplicate_rate() {
        let toml_str = valid_toml_with_faults("duplicate_rate = -0.1");
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_zero_takeoff_altitude() {
        let toml_str = r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 0.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]

            [[assertions]]
            type = "landed"
            timeout_secs = 60
        "#;
        let scenario: ScenarioFile = toml::from_str(toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_zero_acceptance_radius() {
        let wp = "latitude = 47.0\nlongitude = 8.0\naltitude = 10.0\nacceptance_radius = 0.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_negative_acceptance_radius() {
        let wp = "latitude = 47.0\nlongitude = 8.0\naltitude = 10.0\nacceptance_radius = -1.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_invalid_latitude() {
        let wp = "latitude = 91.0\nlongitude = 8.0\naltitude = 10.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_invalid_latitude_negative() {
        let wp = "latitude = -91.0\nlongitude = 8.0\naltitude = 10.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_invalid_longitude() {
        let wp = "latitude = 47.0\nlongitude = 181.0\naltitude = 10.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn reject_invalid_longitude_negative() {
        let wp = "latitude = 47.0\nlongitude = -181.0\naltitude = 10.0";
        let toml_str = valid_toml_with_waypoint(wp);
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn unknown_toml_keys_are_silently_accepted() {
        // Unknown keys are intentionally allowed (deny_unknown_fields is NOT set) so that
        // scenario files written against a newer version of the harness still load correctly
        // on older builds — forward-compatible TOML extension.
        let toml_str = r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]
            unknown_field = "test"

            [[assertions]]
            type = "landed"
            timeout_secs = 60
        "#;
        let result: Result<ScenarioFile, _> = toml::from_str(toml_str);
        assert!(
            result.is_ok(),
            "unknown keys should not cause a parse error"
        );
    }

    #[test]
    fn all_existing_scenarios_parse() {
        // Locate the scenarios/ directory relative to this crate's manifest.
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // The workspace root is two levels up from crates/px4-harness-core/.
        let scenarios_dir = manifest_dir.join("../../scenarios");

        let entries =
            std::fs::read_dir(&scenarios_dir).expect("scenarios/ directory should be readable");

        let mut checked = 0usize;
        for entry in entries {
            let entry = entry.expect("directory entry should be readable");
            let path = entry.path();
            let is_toml = path.extension().and_then(|e| e.to_str()) == Some("toml");
            // Skip suite files — they have a different top-level shape.
            let is_suite = path.file_name().and_then(|n| n.to_str()) == Some("suite.toml");
            if is_toml && !is_suite {
                let result = ScenarioFile::load(&path);
                assert!(
                    result.is_ok(),
                    "scenario file {} failed to load: {:?}",
                    path.display(),
                    result.err()
                );
                checked += 1;
            }
        }

        assert!(
            checked > 0,
            "no .toml files found in scenarios/ — check the path"
        );
    }

    // --- Helper: build a minimal scenario TOML with a custom assertions block ---

    fn minimal_toml_with_assertions(assertions_block: &str) -> String {
        format!(
            r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]

            {assertions_block}
            "#
        )
    }

    // --- Parsing tests for Phase 2 assertion variants ---

    #[test]
    fn parse_segment_timing_assertion() {
        let toml_str = minimal_toml_with_assertions(
            r#"
            [[assertions]]
            type = "segment_timing"
            from_waypoint = 0
            to_waypoint = 1
            max_duration_secs = 30
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.assertions.len(), 1);
        match &scenario.assertions[0] {
            Assertion::SegmentTiming {
                from_waypoint,
                to_waypoint,
                max_duration_secs,
            } => {
                assert_eq!(*from_waypoint, 0);
                assert_eq!(*to_waypoint, 1);
                assert_eq!(*max_duration_secs, 30);
            }
            other => panic!("expected SegmentTiming, got {:?}", other),
        }
    }

    #[test]
    fn parse_geofence_assertion() {
        let toml_str = minimal_toml_with_assertions(
            r#"
            [[assertions]]
            type = "geofence"
            max_altitude = 50.0
            max_distance_m = 100.0
            timeout_secs = 300
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.assertions.len(), 1);
        match &scenario.assertions[0] {
            Assertion::Geofence {
                max_altitude,
                max_distance_m,
                timeout_secs,
            } => {
                assert_eq!(*max_altitude, 50.0);
                assert_eq!(*max_distance_m, 100.0);
                assert_eq!(*timeout_secs, 300);
            }
            other => panic!("expected Geofence, got {:?}", other),
        }
    }

    #[test]
    fn parse_max_tilt_assertion() {
        let toml_str = minimal_toml_with_assertions(
            r#"
            [[assertions]]
            type = "max_tilt"
            max_degrees = 20.0
            timeout_secs = 120
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.assertions.len(), 1);
        match &scenario.assertions[0] {
            Assertion::MaxTilt {
                max_degrees,
                timeout_secs,
            } => {
                assert_eq!(*max_degrees, 20.0);
                assert_eq!(*timeout_secs, 120);
            }
            other => panic!("expected MaxTilt, got {:?}", other),
        }
    }

    #[test]
    fn parse_max_ground_speed_assertion() {
        let toml_str = minimal_toml_with_assertions(
            r#"
            [[assertions]]
            type = "max_ground_speed"
            max_speed_ms = 15.0
            timeout_secs = 180
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.assertions.len(), 1);
        match &scenario.assertions[0] {
            Assertion::MaxGroundSpeed {
                max_speed_ms,
                timeout_secs,
            } => {
                assert_eq!(*max_speed_ms, 15.0);
                assert_eq!(*timeout_secs, 180);
            }
            other => panic!("expected MaxGroundSpeed, got {:?}", other),
        }
    }

    // --- Helper: build minimal TOML with fault_phases appended ---

    fn minimal_toml_with_phases(phases_block: &str) -> String {
        format!(
            r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]

            [[assertions]]
            type = "landed"
            timeout_secs = 60

            {phases_block}
            "#
        )
    }

    // --- Phase 4: multi-vehicle parsing and validation tests ---

    /// Build a minimal valid scenario TOML with a `[[vehicles]]` block appended.
    fn minimal_toml_with_vehicles(vehicles_block: &str) -> String {
        format!(
            r#"
            [scenario]
            name = "Test"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]

            [[assertions]]
            type = "landed"
            timeout_secs = 60

            {vehicles_block}
            "#
        )
    }

    #[test]
    fn parse_vehicles_config() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 1
            px4_port = 14550
            proxy_port = 14560

            [[vehicles]]
            system_id = 2
            px4_port = 14551
            proxy_port = 14561
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.vehicles.len(), 2);
        assert_eq!(scenario.vehicles[0].system_id, 1);
        assert_eq!(scenario.vehicles[0].px4_port, 14550);
        assert_eq!(scenario.vehicles[0].proxy_port, 14560);
        assert_eq!(scenario.vehicles[1].system_id, 2);
        assert_eq!(scenario.vehicles[1].px4_port, 14551);
        assert_eq!(scenario.vehicles[1].proxy_port, 14561);
    }

    #[test]
    fn no_vehicles_backward_compat() {
        let toml_str = r#"
            [scenario]
            name = "Legacy scenario"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]

            [[assertions]]
            type = "landed"
            timeout_secs = 60
        "#;
        let scenario: ScenarioFile = toml::from_str(toml_str).unwrap();
        assert!(
            scenario.vehicles.is_empty(),
            "vehicles should be empty when not specified"
        );
        assert!(
            !scenario.is_multi_vehicle(),
            "is_multi_vehicle() should return false when no vehicles are configured"
        );
    }

    #[test]
    fn reject_duplicate_system_ids() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 1
            px4_port = 14550
            proxy_port = 14560

            [[vehicles]]
            system_id = 1
            px4_port = 14551
            proxy_port = 14561
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(
            result.is_err(),
            "duplicate system_id should fail validation"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("duplicate system_id"),
            "error should mention duplicate system_id, got: {err_msg}"
        );
    }

    #[test]
    fn reject_duplicate_px4_ports() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 1
            px4_port = 14550
            proxy_port = 14560

            [[vehicles]]
            system_id = 2
            px4_port = 14550
            proxy_port = 14561
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(result.is_err(), "duplicate px4_port should fail validation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("duplicate px4_port"),
            "error should mention duplicate px4_port, got: {err_msg}"
        );
    }

    #[test]
    fn reject_duplicate_proxy_ports() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 1
            px4_port = 14550
            proxy_port = 14560

            [[vehicles]]
            system_id = 2
            px4_port = 14551
            proxy_port = 14560
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(
            result.is_err(),
            "duplicate proxy_port should fail validation"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("duplicate proxy_port"),
            "error should mention duplicate proxy_port, got: {err_msg}"
        );
    }

    #[test]
    fn reject_system_id_zero() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 0
            px4_port = 14550
            proxy_port = 14560
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(result.is_err(), "system_id = 0 should fail validation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("broadcast") || err_msg.contains("0"),
            "error should mention broadcast address, got: {err_msg}"
        );
    }

    #[test]
    fn reject_system_id_255() {
        let toml_str = minimal_toml_with_vehicles(
            r#"
            [[vehicles]]
            system_id = 255
            px4_port = 14550
            proxy_port = 14560
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(result.is_err(), "system_id = 255 should fail validation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("GCS") || err_msg.contains("255"),
            "error should mention GCS-reserved address, got: {err_msg}"
        );
    }

    #[test]
    fn parse_min_separation_assertion() {
        let toml_str = minimal_toml_with_assertions(
            r#"
            [[assertions]]
            type = "min_separation"
            min_distance_m = 30.0
            timeout_secs = 60
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.assertions.len(), 1);
        let min_sep = scenario
            .assertions
            .iter()
            .find(|a| matches!(a, Assertion::MinSeparation { .. }))
            .expect("MinSeparation assertion should be present");
        match min_sep {
            Assertion::MinSeparation {
                min_distance_m,
                timeout_secs,
            } => {
                assert_eq!(*min_distance_m, 30.0);
                assert_eq!(*timeout_secs, 60);
            }
            other => panic!("expected MinSeparation, got {:?}", other),
        }
    }

    // --- Phase 3: fault_phases parsing and validation tests ---

    #[test]
    fn parse_fault_phases() {
        let toml_str = minimal_toml_with_phases(
            r#"
            [[fault_phases]]
            after_secs = 30.0
            duration_secs = 15.0
            loss_rate = 0.4
            delay_ms = 200

            [[fault_phases]]
            after_secs = 60.0
            duration_secs = 10.0
            loss_rate = 0.8
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.fault_phases.len(), 2);

        let phase0 = &scenario.fault_phases[0];
        assert_eq!(phase0.after_secs, 30.0);
        assert_eq!(phase0.duration_secs, 15.0);
        assert_eq!(phase0.profile.loss_rate, 0.4);
        assert_eq!(phase0.profile.delay_ms, 200);

        let phase1 = &scenario.fault_phases[1];
        assert_eq!(phase1.after_secs, 60.0);
        assert_eq!(phase1.duration_secs, 10.0);
        assert_eq!(phase1.profile.loss_rate, 0.8);
    }

    #[test]
    fn no_fault_phases_backward_compat() {
        // A scenario with no [[fault_phases]] at all should parse successfully
        // with an empty fault_phases vec, identical behaviour to pre-Phase-3.
        let toml_str = r#"
            [scenario]
            name = "Legacy scenario"

            [mission]
            takeoff_altitude = 10.0

            [[mission.waypoints]]
            latitude = 47.0
            longitude = 8.0
            altitude = 10.0

            [faults]
            loss_rate = 0.1

            [[assertions]]
            type = "landed"
            timeout_secs = 60
        "#;
        let scenario: ScenarioFile = toml::from_str(toml_str).unwrap();
        assert!(
            scenario.fault_phases.is_empty(),
            "fault_phases should be empty when not specified"
        );
        assert_eq!(scenario.faults.loss_rate, 0.1);
        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn reject_fault_phase_zero_duration() {
        let toml_str = minimal_toml_with_phases(
            r#"
            [[fault_phases]]
            after_secs = 10.0
            duration_secs = 0.0
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(
            result.is_err(),
            "duration_secs = 0.0 should fail validation"
        );
    }

    #[test]
    fn reject_fault_phase_negative_after_secs() {
        let toml_str = minimal_toml_with_phases(
            r#"
            [[fault_phases]]
            after_secs = -1.0
            duration_secs = 5.0
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        let result = scenario.validate();
        assert!(result.is_err(), "after_secs = -1.0 should fail validation");
    }

    #[test]
    fn fault_phase_with_all_defaults() {
        // A phase with only after_secs and duration_secs — all fault params default to 0.
        let toml_str = minimal_toml_with_phases(
            r#"
            [[fault_phases]]
            after_secs = 5.0
            duration_secs = 10.0
            "#,
        );
        let scenario: ScenarioFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(scenario.fault_phases.len(), 1);
        let phase = &scenario.fault_phases[0];
        assert_eq!(phase.after_secs, 5.0);
        assert_eq!(phase.duration_secs, 10.0);
        assert_eq!(phase.profile.loss_rate, 0.0);
        assert_eq!(phase.profile.delay_ms, 0);
        assert_eq!(phase.profile.jitter_ms, 0);
        assert_eq!(phase.profile.burst_loss_length, 0);
        assert_eq!(phase.profile.duplicate_rate, 0.0);
        assert_eq!(phase.profile.replay_stale_ms, 0);
        assert!(scenario.validate().is_ok());
    }
}
