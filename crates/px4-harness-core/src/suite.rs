//! Suite runner: loads and groups multiple scenario files for batch execution.

use crate::error::HarnessError;
use crate::scenario::ScenarioFile;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Top-level struct mirroring the suite TOML format.
///
/// ```toml
/// [suite]
/// name = "Fault tolerance regression"
/// description = "optional"
/// scenarios = ["no_faults.toml", "heavy_loss.toml"]
/// ```
#[derive(Debug, Deserialize)]
pub struct SuiteFile {
    pub suite: SuiteConfig,
}

/// Contents of the `[suite]` table.
#[derive(Debug, Deserialize)]
pub struct SuiteConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub scenarios: Vec<PathBuf>,
}

impl SuiteFile {
    /// Read and parse a suite TOML file, then validate it.
    pub fn load(path: &Path) -> Result<Self, HarnessError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| HarnessError::SuiteFileRead {
                path: path.display().to_string(),
                source,
            })?;

        // Re-use the existing ScenarioParse variant — both originate from toml::de::Error.
        let suite: SuiteFile = toml::from_str(&content)?;
        suite.validate()?;
        Ok(suite)
    }

    /// Validate suite constraints:
    /// - `scenarios` list must not be empty.
    /// - No duplicate paths (compared as raw `PathBuf` values).
    pub fn validate(&self) -> Result<(), HarnessError> {
        if self.suite.scenarios.is_empty() {
            return Err(HarnessError::SuiteValidation {
                reason: "suite must list at least one scenario".to_string(),
            });
        }

        let mut seen: HashSet<&PathBuf> = HashSet::new();
        for path in &self.suite.scenarios {
            if !seen.insert(path) {
                return Err(HarnessError::SuiteValidation {
                    reason: format!("duplicate scenario path: {}", path.display()),
                });
            }
        }

        Ok(())
    }

    /// Load every scenario listed in the suite, resolving relative paths against `base_dir`
    /// (typically the directory that contains the suite TOML file).
    ///
    /// Returns each scenario paired with its resolved absolute path.
    /// Fails fast on the first scenario that cannot be loaded.
    pub fn load_scenarios(
        &self,
        base_dir: &Path,
    ) -> Result<Vec<(PathBuf, ScenarioFile)>, HarnessError> {
        self.suite
            .scenarios
            .iter()
            .map(|rel| {
                let abs = base_dir.join(rel);
                let scenario = ScenarioFile::load(&abs)?;
                Ok((abs, scenario))
            })
            .collect()
    }

    /// Discover all `*.toml` files in `dir` (non-recursive) and build a synthetic `SuiteFile`
    /// from them. Paths are sorted alphabetically for deterministic ordering.
    pub fn from_directory(dir: &Path) -> Result<Self, HarnessError> {
        let read_dir = std::fs::read_dir(dir).map_err(|source| HarnessError::SuiteFileRead {
            path: dir.display().to_string(),
            source,
        })?;

        let mut paths: Vec<PathBuf> = read_dir
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        paths.sort();

        let suite = SuiteFile {
            suite: SuiteConfig {
                name: dir.display().to_string(),
                description: None,
                scenarios: paths,
            },
        };

        suite.validate()?;
        Ok(suite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const MINIMAL_SCENARIO: &str = r#"
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
"#;

    fn write_scenario(dir: &TempDir, name: &str) {
        fs::write(dir.path().join(name), MINIMAL_SCENARIO).unwrap();
    }

    fn write_suite(dir: &TempDir, content: &str) -> PathBuf {
        let path = dir.path().join("suite.toml");
        fs::write(&path, content).unwrap();
        path
    }

    // ── SuiteFile::load ────────────────────────────────────────────────────

    #[test]
    fn parse_valid_suite() {
        let tmp = TempDir::new().unwrap();
        write_scenario(&tmp, "a.toml");
        write_scenario(&tmp, "b.toml");

        let suite_content = r#"
[suite]
name = "My Suite"
scenarios = ["a.toml", "b.toml"]
"#;
        let suite_path = write_suite(&tmp, suite_content);
        let suite = SuiteFile::load(&suite_path).unwrap();

        assert_eq!(suite.suite.name, "My Suite");
        assert_eq!(suite.suite.scenarios.len(), 2);
    }

    #[test]
    fn reject_empty_scenarios() {
        let tmp = TempDir::new().unwrap();
        let suite_content = r#"
[suite]
name = "Empty"
scenarios = []
"#;
        let suite_path = write_suite(&tmp, suite_content);
        let err = SuiteFile::load(&suite_path).unwrap_err();
        assert!(
            matches!(err, HarnessError::SuiteValidation { .. }),
            "expected SuiteValidation, got: {err}"
        );
    }

    #[test]
    fn reject_duplicate_scenarios() {
        let tmp = TempDir::new().unwrap();
        let suite_content = r#"
[suite]
name = "Dupes"
scenarios = ["a.toml", "a.toml"]
"#;
        let suite_path = write_suite(&tmp, suite_content);
        let err = SuiteFile::load(&suite_path).unwrap_err();
        assert!(
            matches!(err, HarnessError::SuiteValidation { .. }),
            "expected SuiteValidation, got: {err}"
        );
    }

    #[test]
    fn load_missing_file() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does_not_exist.toml");
        let err = SuiteFile::load(&missing).unwrap_err();
        assert!(
            matches!(err, HarnessError::SuiteFileRead { .. }),
            "expected SuiteFileRead, got: {err}"
        );
    }

    #[test]
    fn single_scenario_suite_works() {
        let tmp = TempDir::new().unwrap();
        write_scenario(&tmp, "only.toml");
        let suite_content = r#"
[suite]
name = "Singleton"
scenarios = ["only.toml"]
"#;
        let suite_path = write_suite(&tmp, suite_content);
        let suite = SuiteFile::load(&suite_path).unwrap();
        assert_eq!(suite.suite.scenarios.len(), 1);
    }

    // ── SuiteFile::load_scenarios ──────────────────────────────────────────

    #[test]
    fn load_scenarios_resolves_relative_paths() {
        let tmp = TempDir::new().unwrap();
        write_scenario(&tmp, "s1.toml");
        write_scenario(&tmp, "s2.toml");

        let suite = SuiteFile {
            suite: SuiteConfig {
                name: "Test Suite".to_string(),
                description: None,
                scenarios: vec![PathBuf::from("s1.toml"), PathBuf::from("s2.toml")],
            },
        };

        let loaded = suite.load_scenarios(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn load_scenarios_fails_on_missing_scenario() {
        let tmp = TempDir::new().unwrap();
        // Only write one of two referenced files.
        write_scenario(&tmp, "present.toml");

        let suite = SuiteFile {
            suite: SuiteConfig {
                name: "Test Suite".to_string(),
                description: None,
                scenarios: vec![PathBuf::from("present.toml"), PathBuf::from("missing.toml")],
            },
        };

        let err = suite.load_scenarios(tmp.path()).unwrap_err();
        assert!(
            matches!(err, HarnessError::ScenarioFileRead { .. }),
            "expected ScenarioFileRead, got: {err}"
        );
    }

    // ── SuiteFile::from_directory ──────────────────────────────────────────

    #[test]
    fn from_directory_discovers_toml_files() {
        let tmp = TempDir::new().unwrap();
        write_scenario(&tmp, "c.toml");
        write_scenario(&tmp, "a.toml");
        write_scenario(&tmp, "b.toml");

        let suite = SuiteFile::from_directory(tmp.path()).unwrap();
        // All 3 found.
        assert_eq!(suite.suite.scenarios.len(), 3);
        // Sorted alphabetically — last segment of each path.
        let names: Vec<_> = suite
            .suite
            .scenarios
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["a.toml", "b.toml", "c.toml"]);
    }

    #[test]
    fn from_directory_empty_rejects() {
        let tmp = TempDir::new().unwrap();
        let err = SuiteFile::from_directory(tmp.path()).unwrap_err();
        assert!(
            matches!(err, HarnessError::SuiteValidation { .. }),
            "expected SuiteValidation, got: {err}"
        );
    }

    #[test]
    fn from_directory_skips_non_toml() {
        let tmp = TempDir::new().unwrap();
        write_scenario(&tmp, "scenario.toml");
        fs::write(tmp.path().join("readme.txt"), "ignore me").unwrap();

        let suite = SuiteFile::from_directory(tmp.path()).unwrap();
        assert_eq!(suite.suite.scenarios.len(), 1);
        assert_eq!(
            suite.suite.scenarios[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "scenario.toml"
        );
    }
}
