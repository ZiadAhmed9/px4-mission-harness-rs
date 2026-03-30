//! Scenario generation utilities: parameter sweeps, binary search, and random fuzzing.
//!
//! These functions generate [`FaultProfile`] values for use in automated scenario
//! generation workflows. They do not run missions — that is the CLI's responsibility.

use crate::scenario::FaultProfile;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Which fault parameter to sweep.
#[derive(Debug, Clone, Copy)]
pub enum SweepParam {
    LossRate,
    DelayMs,
    JitterMs,
    DuplicateRate,
    ReplayStaleMs,
    BurstLossLength,
}

/// Configuration for a parameter sweep.
#[derive(Debug, Clone)]
pub struct SweepConfig {
    pub param: SweepParam,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Base profile — all other params stay at these values.
    pub base_profile: FaultProfile,
}

/// Result of a single generated scenario with its fault profile.
#[derive(Debug, Clone)]
pub struct GeneratedScenario {
    /// Human-readable label (e.g., "loss_rate=0.30")
    pub label: String,
    pub profile: FaultProfile,
    /// Seed used for random generation (None for deterministic sweeps)
    pub seed: Option<u64>,
}

/// Generate a sweep of fault profiles varying one parameter.
///
/// Returns an error if step <= 0 or min > max.
pub fn parameter_sweep(config: &SweepConfig) -> Result<Vec<GeneratedScenario>, String> {
    if config.step <= 0.0 {
        return Err("step must be > 0".to_string());
    }
    if config.min > config.max {
        return Err(format!(
            "min ({}) must be <= max ({})",
            config.min, config.max
        ));
    }

    let mut scenarios = Vec::new();
    let mut value = config.min;

    while value <= config.max + config.step * 0.001 {
        // small epsilon to handle float rounding
        let clamped = value.min(config.max); // don't exceed max
        let mut profile = config.base_profile.clone();
        let label = apply_param(&mut profile, config.param, clamped);

        scenarios.push(GeneratedScenario {
            label,
            profile,
            seed: None,
        });

        value += config.step;
    }

    Ok(scenarios)
}

/// Generate a 2D Cartesian product sweep of two parameters.
pub fn parameter_sweep_2d(
    config_a: &SweepConfig,
    config_b: &SweepConfig,
) -> Result<Vec<GeneratedScenario>, String> {
    let sweep_a = parameter_sweep(config_a)?;
    let sweep_b = parameter_sweep(config_b)?;

    let mut scenarios = Vec::new();
    for a in &sweep_a {
        for b in &sweep_b {
            let mut profile = a.profile.clone();
            let label_b = apply_param(
                &mut profile,
                config_b.param,
                get_param_value(&b.profile, config_b.param),
            );
            scenarios.push(GeneratedScenario {
                label: format!("{} + {}", a.label, label_b),
                profile,
                seed: None,
            });
        }
    }

    Ok(scenarios)
}

/// Configuration for binary search over a fault parameter threshold.
#[derive(Debug, Clone)]
pub struct ThresholdSearchConfig {
    pub param: SweepParam,
    pub min: f64,
    pub max: f64,
    pub tolerance: f64,
    pub max_iterations: u32,
    pub base_profile: FaultProfile,
}

/// Result of a threshold search.
#[derive(Debug, Clone)]
pub enum ThresholdResult {
    /// Found the approximate threshold value.
    Found { value: f64, iterations: u32 },
    /// All values in the range passed (no failure found).
    AllPassed,
    /// All values in the range failed.
    AllFailed,
    /// Results were non-monotonic (flaky).
    Inconclusive { iterations: u32 },
}

/// Generator for binary search over a fault parameter threshold.
///
/// Yields the next profile to test and expects a pass/fail result back.
/// The caller is responsible for running the mission and reporting pass/fail.
pub struct ThresholdSearch {
    config: ThresholdSearchConfig,
    low: f64,
    high: f64,
    iteration: u32,
}

impl ThresholdSearch {
    pub fn new(config: ThresholdSearchConfig) -> Self {
        Self {
            low: config.min,
            high: config.max,
            iteration: 0,
            config,
        }
    }

    /// Get the next profile to test. Returns None if search is complete.
    pub fn next_profile(&self) -> Option<(f64, FaultProfile)> {
        if self.iteration >= self.config.max_iterations {
            return None;
        }
        if (self.high - self.low) < self.config.tolerance {
            return None;
        }
        let mid = (self.low + self.high) / 2.0;
        let mut profile = self.config.base_profile.clone();
        apply_param(&mut profile, self.config.param, mid);
        Some((mid, profile))
    }

    /// Report the result of running the mission at the current midpoint.
    ///
    /// `passed` = true means the mission passed (search goes higher),
    /// `passed` = false means it failed (search goes lower).
    pub fn report_result(&mut self, passed: bool) {
        let mid = (self.low + self.high) / 2.0;
        if passed {
            self.low = mid;
        } else {
            self.high = mid;
        }
        self.iteration += 1;
    }

    /// Get the current best estimate of the threshold.
    pub fn current_estimate(&self) -> f64 {
        (self.low + self.high) / 2.0
    }

    /// Get the number of iterations completed.
    pub fn iterations(&self) -> u32 {
        self.iteration
    }

    /// Check if the search has converged.
    pub fn is_converged(&self) -> bool {
        (self.high - self.low) < self.config.tolerance
            || self.iteration >= self.config.max_iterations
    }
}

/// Configuration for random fuzzing.
#[derive(Debug, Clone)]
pub struct FuzzConfig {
    pub num_scenarios: u32,
    pub seed: u64,
    /// Bounds for each parameter: (min, max). If None, parameter stays at base value.
    pub loss_rate_range: Option<(f64, f64)>,
    pub delay_ms_range: Option<(u64, u64)>,
    pub jitter_ms_range: Option<(u64, u64)>,
    pub duplicate_rate_range: Option<(f64, f64)>,
    pub replay_stale_ms_range: Option<(u64, u64)>,
    pub burst_loss_length_range: Option<(u32, u32)>,
    pub base_profile: FaultProfile,
}

/// Generate random fault profiles within the configured bounds.
pub fn random_fuzz(config: &FuzzConfig) -> Result<Vec<GeneratedScenario>, String> {
    if config.num_scenarios == 0 {
        return Err("num_scenarios must be > 0".to_string());
    }

    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut scenarios = Vec::new();

    for i in 0..config.num_scenarios {
        let mut profile = config.base_profile.clone();
        let iter_seed = config.seed.wrapping_add(i as u64);

        if let Some((min, max)) = config.loss_rate_range {
            profile.loss_rate = rng.random_range(min..=max);
        }
        if let Some((min, max)) = config.delay_ms_range {
            profile.delay_ms = rng.random_range(min..=max);
        }
        if let Some((min, max)) = config.jitter_ms_range {
            profile.jitter_ms = rng.random_range(min..=max);
        }
        if let Some((min, max)) = config.duplicate_rate_range {
            profile.duplicate_rate = rng.random_range(min..=max);
        }
        if let Some((min, max)) = config.replay_stale_ms_range {
            profile.replay_stale_ms = rng.random_range(min..=max);
        }
        if let Some((min, max)) = config.burst_loss_length_range {
            profile.burst_loss_length = rng.random_range(min..=max);
        }

        scenarios.push(GeneratedScenario {
            label: format!("fuzz_{}", i),
            profile,
            seed: Some(iter_seed),
        });
    }

    Ok(scenarios)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_profile() -> FaultProfile {
        FaultProfile {
            delay_ms: 0,
            jitter_ms: 0,
            loss_rate: 0.0,
            burst_loss_length: 0,
            duplicate_rate: 0.0,
            replay_stale_ms: 0,
        }
    }

    // ---- Parameter Sweep Tests ----

    #[test]
    fn sweep_loss_rate_basic() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 0.5,
            step: 0.1,
            base_profile: base_profile(),
        };
        let scenarios = parameter_sweep(&config).expect("should succeed");
        assert_eq!(scenarios.len(), 6, "expected 6 scenarios");
        let first = &scenarios[0];
        let last = &scenarios[5];
        assert!(
            (first.profile.loss_rate - 0.0).abs() < 1e-9,
            "first loss_rate should be 0.0, got {}",
            first.profile.loss_rate
        );
        assert!(
            (last.profile.loss_rate - 0.5).abs() < 1e-9,
            "last loss_rate should be 0.5, got {}",
            last.profile.loss_rate
        );
    }

    #[test]
    fn sweep_rejects_zero_step() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            step: 0.0,
            base_profile: base_profile(),
        };
        let result = parameter_sweep(&config);
        assert!(result.is_err(), "expected error for zero step");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("step"),
            "error message should mention 'step', got: {}",
            msg
        );
    }

    #[test]
    fn sweep_rejects_min_greater_than_max() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.5,
            max: 0.1,
            step: 0.1,
            base_profile: base_profile(),
        };
        let result = parameter_sweep(&config);
        assert!(result.is_err(), "expected error when min > max");
    }

    #[test]
    fn sweep_single_value() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.3,
            max: 0.3,
            step: 0.1,
            base_profile: base_profile(),
        };
        let scenarios = parameter_sweep(&config).expect("should succeed");
        assert_eq!(scenarios.len(), 1, "expected exactly 1 scenario");
        assert!(
            (scenarios[0].profile.loss_rate - 0.3).abs() < 1e-9,
            "loss_rate should be 0.3, got {}",
            scenarios[0].profile.loss_rate
        );
    }

    #[test]
    fn sweep_step_larger_than_range() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 0.05,
            step: 0.1,
            base_profile: base_profile(),
        };
        let scenarios = parameter_sweep(&config).expect("should succeed");
        assert_eq!(
            scenarios.len(),
            1,
            "expected exactly 1 scenario (just the min)"
        );
        assert!(
            (scenarios[0].profile.loss_rate - 0.0).abs() < 1e-9,
            "loss_rate should be 0.0, got {}",
            scenarios[0].profile.loss_rate
        );
    }

    #[test]
    fn sweep_all_profiles_valid() {
        let config = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            base_profile: base_profile(),
        };
        let scenarios = parameter_sweep(&config).expect("should succeed");
        for s in &scenarios {
            assert!(
                s.profile.loss_rate >= 0.0 && s.profile.loss_rate <= 1.0,
                "loss_rate {} out of [0.0, 1.0]",
                s.profile.loss_rate
            );
        }
    }

    #[test]
    fn sweep_2d_cartesian_product() {
        let config_a = SweepConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 0.2,
            step: 0.1,
            base_profile: base_profile(),
        };
        let config_b = SweepConfig {
            param: SweepParam::DelayMs,
            min: 0.0,
            max: 200.0,
            step: 100.0,
            base_profile: base_profile(),
        };
        let scenarios = parameter_sweep_2d(&config_a, &config_b).expect("should succeed");
        assert_eq!(scenarios.len(), 9, "expected 9 scenarios (3x3 product)");

        // Build the expected (loss_rate, delay_ms) pairs
        let expected_loss_rates = [0.0f64, 0.1, 0.2];
        let expected_delays = [0u64, 100, 200];
        let mut expected_pairs: Vec<(f64, u64)> = Vec::new();
        for &lr in &expected_loss_rates {
            for &d in &expected_delays {
                expected_pairs.push((lr, d));
            }
        }

        for (i, s) in scenarios.iter().enumerate() {
            let (expected_lr, expected_d) = expected_pairs[i];
            assert!(
                (s.profile.loss_rate - expected_lr).abs() < 1e-9,
                "scenario {}: expected loss_rate={}, got {}",
                i,
                expected_lr,
                s.profile.loss_rate
            );
            assert_eq!(
                s.profile.delay_ms, expected_d,
                "scenario {}: expected delay_ms={}, got {}",
                i, expected_d, s.profile.delay_ms
            );
        }
    }

    // ---- Threshold Search Tests ----

    #[test]
    fn threshold_search_converges() {
        let config = ThresholdSearchConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            tolerance: 0.01,
            max_iterations: 20,
            base_profile: base_profile(),
        };
        let mut search = ThresholdSearch::new(config);

        loop {
            match search.next_profile() {
                None => break,
                Some((value, _profile)) => {
                    let passed = value < 0.25;
                    search.report_result(passed);
                }
            }
        }

        assert!(search.is_converged(), "search should have converged");
        let estimate = search.current_estimate();
        assert!(
            (estimate - 0.25).abs() <= 0.01,
            "estimate {} should be within 0.01 of 0.25",
            estimate
        );
    }

    #[test]
    fn threshold_search_all_pass() {
        let config = ThresholdSearchConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            tolerance: 0.001,
            max_iterations: 20,
            base_profile: base_profile(),
        };
        let mut search = ThresholdSearch::new(config);

        loop {
            match search.next_profile() {
                None => break,
                Some(_) => search.report_result(true),
            }
        }

        // When everything passes, low bound should be pushed towards max
        assert!(
            search.low > 0.9,
            "low bound {} should be near 1.0 when all pass",
            search.low
        );
    }

    #[test]
    fn threshold_search_all_fail() {
        let config = ThresholdSearchConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            tolerance: 0.001,
            max_iterations: 20,
            base_profile: base_profile(),
        };
        let mut search = ThresholdSearch::new(config);

        loop {
            match search.next_profile() {
                None => break,
                Some(_) => search.report_result(false),
            }
        }

        // When everything fails, high bound should be pushed towards min
        assert!(
            search.high < 0.1,
            "high bound {} should be near 0.0 when all fail",
            search.high
        );
    }

    #[test]
    fn threshold_search_respects_max_iterations() {
        let config = ThresholdSearchConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            tolerance: 0.0001,
            max_iterations: 5,
            base_profile: base_profile(),
        };
        let mut search = ThresholdSearch::new(config);
        let mut count = 0;

        loop {
            match search.next_profile() {
                None => break,
                Some((value, _)) => {
                    count += 1;
                    let passed = value < 0.5;
                    search.report_result(passed);
                }
            }
        }

        assert_eq!(
            count, 5,
            "should stop after exactly 5 iterations, got {}",
            count
        );
    }

    #[test]
    fn threshold_search_respects_tolerance() {
        // With tolerance=0.5 on a 0.0-1.0 range, the range (1.0) is already > tolerance.
        // After one bisection the range becomes 0.5 which equals tolerance,
        // so convergence should happen very quickly.
        let config = ThresholdSearchConfig {
            param: SweepParam::LossRate,
            min: 0.0,
            max: 1.0,
            tolerance: 0.5,
            max_iterations: 20,
            base_profile: base_profile(),
        };
        let mut search = ThresholdSearch::new(config);
        let mut count = 0;

        loop {
            match search.next_profile() {
                None => break,
                Some((value, _)) => {
                    count += 1;
                    let passed = value < 0.5;
                    search.report_result(passed);
                }
            }
        }

        assert!(
            count <= 2,
            "should converge in at most 2 iterations with tolerance=0.5, took {}",
            count
        );
    }

    // ---- Random Fuzzing Tests ----

    #[test]
    fn fuzz_generates_correct_count() {
        let config = FuzzConfig {
            num_scenarios: 100,
            seed: 42,
            loss_rate_range: Some((0.0, 1.0)),
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };
        let scenarios = random_fuzz(&config).expect("should succeed");
        assert_eq!(scenarios.len(), 100, "expected exactly 100 scenarios");
    }

    #[test]
    fn fuzz_rejects_zero_scenarios() {
        let config = FuzzConfig {
            num_scenarios: 0,
            seed: 42,
            loss_rate_range: None,
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };
        let result = random_fuzz(&config);
        assert!(result.is_err(), "expected error for num_scenarios=0");
    }

    #[test]
    fn fuzz_seed_reproducibility() {
        let make_config = || FuzzConfig {
            num_scenarios: 50,
            seed: 1234,
            loss_rate_range: Some((0.0, 1.0)),
            delay_ms_range: Some((0, 500)),
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };

        let run1 = random_fuzz(&make_config()).expect("run1 should succeed");
        let run2 = random_fuzz(&make_config()).expect("run2 should succeed");

        assert_eq!(run1.len(), run2.len());
        for (a, b) in run1.iter().zip(run2.iter()) {
            assert_eq!(
                a.profile.loss_rate, b.profile.loss_rate,
                "loss_rate should be identical across runs"
            );
            assert_eq!(
                a.profile.delay_ms, b.profile.delay_ms,
                "delay_ms should be identical across runs"
            );
        }
    }

    #[test]
    fn fuzz_different_seeds_differ() {
        let config_42 = FuzzConfig {
            num_scenarios: 50,
            seed: 42,
            loss_rate_range: Some((0.0, 1.0)),
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };
        let config_99 = FuzzConfig {
            seed: 99,
            ..config_42.clone()
        };

        let run42 = random_fuzz(&config_42).expect("seed 42 should succeed");
        let run99 = random_fuzz(&config_99).expect("seed 99 should succeed");

        let any_differ = run42
            .iter()
            .zip(run99.iter())
            .any(|(a, b)| (a.profile.loss_rate - b.profile.loss_rate).abs() > 1e-12);

        assert!(any_differ, "outputs with different seeds should differ");
    }

    #[test]
    fn fuzz_respects_bounds() {
        let config = FuzzConfig {
            num_scenarios: 50,
            seed: 7,
            loss_rate_range: Some((0.1, 0.3)),
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };
        let scenarios = random_fuzz(&config).expect("should succeed");
        for s in &scenarios {
            assert!(
                s.profile.loss_rate >= 0.1 && s.profile.loss_rate <= 0.3,
                "loss_rate {} out of [0.1, 0.3]",
                s.profile.loss_rate
            );
        }
    }

    #[test]
    fn fuzz_unset_params_stay_at_base() {
        let mut base = base_profile();
        base.delay_ms = 100;

        let config = FuzzConfig {
            num_scenarios: 20,
            seed: 55,
            loss_rate_range: Some((0.0, 1.0)),
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base,
        };
        let scenarios = random_fuzz(&config).expect("should succeed");
        for s in &scenarios {
            assert_eq!(
                s.profile.delay_ms, 100,
                "delay_ms should stay at base value 100, got {}",
                s.profile.delay_ms
            );
        }
    }

    #[test]
    fn fuzz_records_seed() {
        let config = FuzzConfig {
            num_scenarios: 10,
            seed: 999,
            loss_rate_range: None,
            delay_ms_range: None,
            jitter_ms_range: None,
            duplicate_rate_range: None,
            replay_stale_ms_range: None,
            burst_loss_length_range: None,
            base_profile: base_profile(),
        };
        let scenarios = random_fuzz(&config).expect("should succeed");
        for s in &scenarios {
            assert!(
                s.seed.is_some(),
                "each generated scenario should have seed=Some(value)"
            );
        }
    }
}

// --- Internal helpers ---

/// Apply a parameter value to a profile and return a label string.
fn apply_param(profile: &mut FaultProfile, param: SweepParam, value: f64) -> String {
    match param {
        SweepParam::LossRate => {
            profile.loss_rate = value;
            format!("loss_rate={:.2}", value)
        }
        SweepParam::DelayMs => {
            profile.delay_ms = value as u64;
            format!("delay_ms={}", value as u64)
        }
        SweepParam::JitterMs => {
            profile.jitter_ms = value as u64;
            format!("jitter_ms={}", value as u64)
        }
        SweepParam::DuplicateRate => {
            profile.duplicate_rate = value;
            format!("duplicate_rate={:.2}", value)
        }
        SweepParam::ReplayStaleMs => {
            profile.replay_stale_ms = value as u64;
            format!("replay_stale_ms={}", value as u64)
        }
        SweepParam::BurstLossLength => {
            profile.burst_loss_length = value as u32;
            format!("burst_loss_length={}", value as u32)
        }
    }
}

/// Read a parameter value from a profile.
fn get_param_value(profile: &FaultProfile, param: SweepParam) -> f64 {
    match param {
        SweepParam::LossRate => profile.loss_rate,
        SweepParam::DelayMs => profile.delay_ms as f64,
        SweepParam::JitterMs => profile.jitter_ms as f64,
        SweepParam::DuplicateRate => profile.duplicate_rate,
        SweepParam::ReplayStaleMs => profile.replay_stale_ms as f64,
        SweepParam::BurstLossLength => profile.burst_loss_length as f64,
    }
}
