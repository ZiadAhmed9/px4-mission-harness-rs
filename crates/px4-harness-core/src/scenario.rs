//Rust structs that mirror the TOML format. Serde will automatically convert between TOML and these types.

use crate::error::HarnessError;
use serde::Deserialize;
use std::path::Path;

//Struct of the entire scenario file, which includes metadata, mission details, fault profiles, and assertions.
#[derive(Debug, Deserialize)]
pub struct ScenarioFile {
    pub scenario: ScenarioMeta,
    pub mission: Mission,
    pub faults: FaultProfile,
    pub assertions: Vec<Assertion>,
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

#[derive(Debug, Deserialize)]
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
        Ok(())
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
}
