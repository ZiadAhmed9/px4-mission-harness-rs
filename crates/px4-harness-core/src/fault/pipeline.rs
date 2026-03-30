use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::Rng;
use serde::Serialize;

use crate::scenario::{FaultPhase, FaultProfile};

/// Counters for fault pipeline activity.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FaultStats {
    pub packets_processed: u64,
    pub packets_forwarded: u64,
    pub packets_dropped: u64,
    pub packets_duplicated: u64,
    pub packets_replayed: u64,
}

/// What to do with a packet after passing through the fault pipeline.
pub enum FaultAction {
    /// Forward the packet after the specified delay
    Forward { data: Vec<u8>, delay: Duration },
    /// Drop the packet entirely
    Drop,
}

/// Processes raw UDP packets through a series of fault stages:
/// Drop → Delay/Jitter → Duplicate → Replay
///
/// Supports an optional list of time-based `FaultPhase` entries. When the current
/// elapsed time falls within a phase's window, that phase's profile overrides the
/// default. When multiple phases overlap, the last matching phase wins.
pub struct FaultPipeline {
    /// Default/static fault profile (used when no phase is active)
    pub default_profile: FaultProfile,
    /// Time-based fault phases (sorted by after_secs for deterministic resolution)
    phases: Vec<FaultPhase>,
    /// When the pipeline was started (for elapsed time calculations)
    start_time: Instant,
    /// Recent packets stored for potential stale replay
    replay_buffer: VecDeque<(Instant, Vec<u8>)>,
    /// When > 0, drop packets unconditionally (burst loss)
    pub burst_remaining: u32,
    /// Running counters of pipeline activity
    stats: FaultStats,
}

impl FaultPipeline {
    /// Create a pipeline with only a static default profile and no phases.
    pub fn new(default_profile: FaultProfile) -> Self {
        Self {
            default_profile,
            phases: Vec::new(),
            start_time: Instant::now(),
            replay_buffer: VecDeque::with_capacity(100),
            burst_remaining: 0,
            stats: FaultStats::default(),
        }
    }

    /// Create a pipeline with a default profile and a set of time-based phases.
    /// Phases are sorted by `after_secs` for deterministic overlap resolution.
    pub fn with_phases(default_profile: FaultProfile, mut phases: Vec<FaultPhase>) -> Self {
        phases.sort_by(|a, b| {
            a.after_secs
                .partial_cmp(&b.after_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self {
            default_profile,
            phases,
            start_time: Instant::now(),
            replay_buffer: VecDeque::with_capacity(100),
            burst_remaining: 0,
            stats: FaultStats::default(),
        }
    }

    /// Get the active fault profile based on current elapsed time.
    ///
    /// If any phase is currently active (`after_secs <= elapsed < after_secs + duration_secs`),
    /// returns the last matching phase's profile. Otherwise returns the default profile.
    fn active_profile(&self) -> &FaultProfile {
        let elapsed = self.start_time.elapsed().as_secs_f64();

        // Iterate in reverse to find the last (highest-priority) active phase
        let active = self.phases.iter().rev().find(|phase| {
            elapsed >= phase.after_secs && elapsed < phase.after_secs + phase.duration_secs
        });

        match active {
            Some(phase) => &phase.profile,
            None => &self.default_profile,
        }
    }

    /// Process a packet through the fault pipeline.
    /// Returns zero or more actions — a single packet may produce:
    /// - 0 actions (dropped)
    /// - 1 action (normal forward)
    /// - 2+ actions (duplicated and/or replayed)
    pub fn process(&mut self, data: &[u8]) -> Vec<FaultAction> {
        self.stats.packets_processed += 1;

        let profile = self.active_profile().clone();
        let mut rng = rand::rng();

        // Store in replay buffer for potential future replay
        self.replay_buffer
            .push_back((Instant::now(), data.to_vec()));
        if self.replay_buffer.len() > 100 {
            self.replay_buffer.pop_front();
        }

        // Stage 1: Burst drop — if in a burst, drop unconditionally
        if self.burst_remaining > 0 {
            self.burst_remaining -= 1;
            self.stats.packets_dropped += 1;
            return vec![FaultAction::Drop];
        }

        // Stage 2: Random drop
        if profile.loss_rate > 0.0 && rng.random::<f64>() < profile.loss_rate {
            // Start a burst if configured
            if profile.burst_loss_length > 1 {
                self.burst_remaining = profile.burst_loss_length - 1;
            }
            self.stats.packets_dropped += 1;
            return vec![FaultAction::Drop];
        }

        // Stage 3: Calculate delay (fixed + random jitter)
        let base_delay = Duration::from_millis(profile.delay_ms);
        let jitter = if profile.jitter_ms > 0 {
            Duration::from_millis(rng.random_range(0..=profile.jitter_ms))
        } else {
            Duration::ZERO
        };
        let total_delay = base_delay + jitter;

        let mut actions = vec![];

        // Stage 4: Forward the original packet
        actions.push(FaultAction::Forward {
            data: data.to_vec(),
            delay: total_delay,
        });
        self.stats.packets_forwarded += 1;

        // Stage 5: Duplicate check — send the packet a second time
        if profile.duplicate_rate > 0.0 && rng.random::<f64>() < profile.duplicate_rate {
            let dup_jitter = if profile.jitter_ms > 0 {
                Duration::from_millis(rng.random_range(0..=profile.jitter_ms))
            } else {
                Duration::ZERO
            };
            actions.push(FaultAction::Forward {
                data: data.to_vec(),
                delay: total_delay + dup_jitter,
            });
            self.stats.packets_duplicated += 1;
        }

        // Stage 6: Replay stale — inject an old packet from N ms ago
        if profile.replay_stale_ms > 0 {
            let stale_threshold = Duration::from_millis(profile.replay_stale_ms);
            let now = Instant::now();
            // Find the oldest packet that's at least replay_stale_ms old
            if let Some((_, stale_data)) = self
                .replay_buffer
                .iter()
                .find(|(ts, _)| now.duration_since(*ts) >= stale_threshold)
            {
                actions.push(FaultAction::Forward {
                    data: stale_data.clone(),
                    delay: total_delay,
                });
                self.stats.packets_replayed += 1;
            }
        }

        actions
    }

    /// Returns true if any fault is currently enabled (default profile, any phase, or phases present).
    pub fn is_active(&self) -> bool {
        let p = self.active_profile();
        p.delay_ms > 0
            || p.jitter_ms > 0
            || p.loss_rate > 0.0
            || p.burst_loss_length > 0
            || p.duplicate_rate > 0.0
            || p.replay_stale_ms > 0
            || !self.phases.is_empty()
    }

    /// Get a snapshot of the current statistics.
    pub fn stats(&self) -> &FaultStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::FaultProfile;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn zero_loss_never_drops_prop(
            delay_ms in 0u64..1000,
            jitter_ms in 0u64..500,
            duplicate_rate in 0.0f64..1.0,
        ) {
            let profile = FaultProfile {
                delay_ms,
                jitter_ms,
                loss_rate: 0.0,
                burst_loss_length: 0,
                duplicate_rate,
                replay_stale_ms: 0,
            };
            let mut pipeline = FaultPipeline::new(profile);
            let actions = pipeline.process(b"test");
            // With loss_rate=0, we should always get at least one Forward
            let has_forward = actions.iter().any(|a| matches!(a, FaultAction::Forward { .. }));
            prop_assert!(has_forward);
        }

        #[test]
        fn actions_count_is_bounded(
            loss_rate in 0.0f64..=1.0,
            duplicate_rate in 0.0f64..=1.0,
            replay_stale_ms in 0u64..100,
        ) {
            let profile = FaultProfile {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate,
                burst_loss_length: 0,
                duplicate_rate,
                replay_stale_ms,
            };
            let mut pipeline = FaultPipeline::new(profile);
            let actions = pipeline.process(b"test");
            // Max possible: 1 forward + 1 duplicate + 1 replay = 3, or 1 drop
            prop_assert!(actions.len() <= 3, "too many actions: {}", actions.len());
        }

        #[test]
        fn full_loss_never_forwards(
            burst_loss_length in 0u32..10,
            duplicate_rate in 0.0f64..=1.0,
        ) {
            let profile = FaultProfile {
                delay_ms: 0,
                jitter_ms: 0,
                loss_rate: 1.0,
                burst_loss_length,
                duplicate_rate,
                replay_stale_ms: 0,
            };
            let mut pipeline = FaultPipeline::new(profile);
            let actions = pipeline.process(b"test");
            // With 100% loss, every packet should be dropped
            prop_assert!(actions.iter().all(|a| matches!(a, FaultAction::Drop)));
        }
    }

    fn no_fault_profile() -> FaultProfile {
        FaultProfile {
            delay_ms: 0,
            jitter_ms: 0,
            loss_rate: 0.0,
            burst_loss_length: 0,
            duplicate_rate: 0.0,
            replay_stale_ms: 0,
        }
    }

    #[test]
    fn no_faults_forwards_packet() {
        let mut pipeline = FaultPipeline::new(no_fault_profile());
        let actions = pipeline.process(b"test packet");
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], FaultAction::Forward { delay, .. } if delay.is_zero()));
    }

    #[test]
    fn full_loss_drops_everything() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 1.0;
        let mut pipeline = FaultPipeline::new(profile);
        let actions = pipeline.process(b"test");
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], FaultAction::Drop));
    }

    #[test]
    fn delay_is_applied() {
        let mut profile = no_fault_profile();
        profile.delay_ms = 100;
        let mut pipeline = FaultPipeline::new(profile);
        let actions = pipeline.process(b"test");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            FaultAction::Forward { delay, .. } => {
                assert!(delay.as_millis() >= 100);
            }
            _ => panic!("expected Forward"),
        }
    }

    #[test]
    fn full_duplicate_sends_twice() {
        let mut profile = no_fault_profile();
        profile.duplicate_rate = 1.0;
        let mut pipeline = FaultPipeline::new(profile);
        let actions = pipeline.process(b"test");
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn burst_loss_drops_consecutive() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 1.0;
        profile.burst_loss_length = 3;
        let mut pipeline = FaultPipeline::new(profile);

        // First packet triggers the burst
        let a1 = pipeline.process(b"pkt1");
        assert!(matches!(a1[0], FaultAction::Drop));

        // Next 2 packets are burst-dropped (even though we reset loss_rate check)
        let a2 = pipeline.process(b"pkt2");
        assert!(matches!(a2[0], FaultAction::Drop));

        let a3 = pipeline.process(b"pkt3");
        assert!(matches!(a3[0], FaultAction::Drop));
    }

    #[test]
    fn pipeline_inactive_with_no_faults() {
        let pipeline = FaultPipeline::new(no_fault_profile());
        assert!(!pipeline.is_active());
    }

    #[test]
    fn pipeline_active_with_loss() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 0.1;
        let pipeline = FaultPipeline::new(profile);
        assert!(pipeline.is_active());
    }

    #[test]
    fn zero_loss_rate_never_drops() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 0.0;
        let mut pipeline = FaultPipeline::new(profile);

        for i in 0u32..1000 {
            let pkt = i.to_le_bytes();
            let actions = pipeline.process(&pkt);
            assert_eq!(
                actions.len(),
                1,
                "packet {i} produced {} actions, expected 1",
                actions.len()
            );
            assert!(
                matches!(&actions[0], FaultAction::Forward { .. }),
                "packet {i} was not forwarded"
            );
        }
    }

    #[test]
    fn full_loss_no_burst() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 1.0;
        profile.burst_loss_length = 0;
        let mut pipeline = FaultPipeline::new(profile);

        // First packet should be dropped
        let actions = pipeline.process(b"pkt1");
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], FaultAction::Drop));

        // burst_remaining must still be 0 — burst_loss_length=0 means no burst started
        assert_eq!(
            pipeline.burst_remaining, 0,
            "burst_remaining should be 0 when burst_loss_length=0"
        );

        // Now disable loss: next packet should be forwarded (no lingering burst)
        pipeline.default_profile.loss_rate = 0.0;
        let actions2 = pipeline.process(b"pkt2");
        assert_eq!(actions2.len(), 1);
        assert!(
            matches!(&actions2[0], FaultAction::Forward { .. }),
            "packet after disabling loss_rate should be forwarded"
        );
    }

    #[test]
    fn burst_length_one_no_extra_drops() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 1.0;
        profile.burst_loss_length = 1;
        let mut pipeline = FaultPipeline::new(profile);

        // First packet: random drop triggers. burst_loss_length=1 means the condition
        // `burst_loss_length > 1` is false, so burst_remaining is NOT incremented.
        let a1 = pipeline.process(b"pkt1");
        assert_eq!(a1.len(), 1);
        assert!(matches!(a1[0], FaultAction::Drop));

        // burst_remaining must still be 0 after the first drop
        assert_eq!(
            pipeline.burst_remaining, 0,
            "burst_remaining should be 0 after a drop with burst_loss_length=1"
        );

        // Second packet: also dropped because loss_rate is still 1.0, but NOT due to burst
        let a2 = pipeline.process(b"pkt2");
        assert_eq!(a2.len(), 1);
        assert!(matches!(a2[0], FaultAction::Drop));

        // burst_remaining is still 0 — second drop also didn't start a burst
        assert_eq!(
            pipeline.burst_remaining, 0,
            "burst_remaining should still be 0 after second drop"
        );
    }

    #[test]
    fn replay_buffer_capacity_bounded() {
        let mut profile = no_fault_profile();
        profile.replay_stale_ms = 1;
        let mut pipeline = FaultPipeline::new(profile);

        for i in 0u32..150 {
            let pkt = i.to_le_bytes();
            pipeline.process(&pkt);
        }

        assert!(
            pipeline.replay_buffer.len() <= 100,
            "replay_buffer grew to {} entries, max is 100",
            pipeline.replay_buffer.len()
        );
    }

    #[test]
    fn replay_stale_first_packet_no_panic() {
        let mut profile = no_fault_profile();
        profile.replay_stale_ms = 100;
        let mut pipeline = FaultPipeline::new(profile);

        // The first packet is 0ms old, so replay won't find a stale packet.
        // Expect exactly 1 Forward and no panic.
        let actions = pipeline.process(b"first packet");
        assert!(!actions.is_empty(), "should return at least 1 action");
        assert!(
            matches!(&actions[0], FaultAction::Forward { .. }),
            "first action should be Forward"
        );
    }

    #[test]
    fn all_three_actions_combined() {
        use std::thread;

        let mut profile = no_fault_profile();
        profile.duplicate_rate = 1.0;
        profile.replay_stale_ms = 1;
        profile.loss_rate = 0.0;
        let mut pipeline = FaultPipeline::new(profile);

        // Fill the replay buffer with a few packets so there is something stale
        for i in 0u8..5 {
            pipeline.process(&[i]);
        }

        // Wait long enough that those packets are older than replay_stale_ms=1ms
        thread::sleep(Duration::from_millis(2));

        // Process one more packet: should produce Forward (original) + Forward (duplicate)
        // + Forward (replay of a stale packet) = 3 actions
        let actions = pipeline.process(b"trigger");
        assert_eq!(
            actions.len(),
            3,
            "expected 3 Forward actions (original + duplicate + replay), got {}",
            actions.len()
        );
        for (i, action) in actions.iter().enumerate() {
            assert!(
                matches!(action, FaultAction::Forward { .. }),
                "action {i} should be Forward"
            );
        }
    }

    #[test]
    fn zero_length_packet_no_panic() {
        let mut pipeline = FaultPipeline::new(no_fault_profile());
        let actions = pipeline.process(b"");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            FaultAction::Forward { data, delay } => {
                assert!(
                    data.is_empty(),
                    "data should be empty for zero-length packet"
                );
                assert!(
                    delay.is_zero(),
                    "delay should be zero with no fault profile"
                );
            }
            FaultAction::Drop => panic!("zero-length packet should not be dropped"),
        }
    }

    // --- Phase 3: dynamic fault injection tests ---

    use crate::scenario::FaultPhase;

    fn phase_with_loss(after_secs: f64, duration_secs: f64, loss_rate: f64) -> FaultPhase {
        FaultPhase {
            after_secs,
            duration_secs,
            profile: FaultProfile {
                loss_rate,
                ..no_fault_profile()
            },
        }
    }

    #[test]
    fn active_profile_uses_default_when_no_phases() {
        let pipeline = FaultPipeline::new(no_fault_profile());
        let profile = pipeline.active_profile();
        assert_eq!(profile.loss_rate, 0.0);
        assert_eq!(profile.delay_ms, 0);
    }

    #[test]
    fn active_profile_returns_phase_when_active() {
        // Phase starts at 0.0s so it is active immediately.
        let phase = phase_with_loss(0.0, 10.0, 0.5);
        let pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase]);
        let profile = pipeline.active_profile();
        assert_eq!(
            profile.loss_rate, 0.5,
            "active phase should override default"
        );
    }

    #[test]
    fn active_profile_returns_default_after_phase_expires() {
        use std::thread;

        // Phase lasts only 10ms starting at t=0.
        let phase = phase_with_loss(0.0, 0.01, 0.9);
        let pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase]);

        // Wait for the phase to expire.
        thread::sleep(Duration::from_millis(20));

        let profile = pipeline.active_profile();
        assert_eq!(
            profile.loss_rate, 0.0,
            "expired phase should fall back to default"
        );
    }

    #[test]
    fn phase_before_start_uses_default() {
        // Phase starts 1000 seconds in the future — should never be active now.
        let phase = phase_with_loss(1000.0, 10.0, 0.99);
        let pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase]);
        let profile = pipeline.active_profile();
        assert_eq!(
            profile.loss_rate, 0.0,
            "future phase should not be active yet"
        );
    }

    #[test]
    fn overlapping_phases_last_wins() {
        // Both phases start at 0.0s and last 10s. After sorting by after_secs (equal),
        // the second element in the input vec will be last in the sorted order (stable sort).
        // active_profile() iterates in reverse and returns the first match, which is phase B.
        let phase_a = FaultPhase {
            after_secs: 0.0,
            duration_secs: 10.0,
            profile: FaultProfile {
                loss_rate: 0.3,
                ..no_fault_profile()
            },
        };
        let phase_b = FaultPhase {
            after_secs: 0.0,
            duration_secs: 10.0,
            profile: FaultProfile {
                loss_rate: 0.7,
                ..no_fault_profile()
            },
        };
        let pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase_a, phase_b]);
        let profile = pipeline.active_profile();
        assert_eq!(
            profile.loss_rate, 0.7,
            "last phase (phase_b) should win on overlap"
        );
    }

    #[test]
    fn pipeline_with_phases_is_active() {
        // Even when default has no faults, presence of phases makes is_active() true.
        let phase = phase_with_loss(1000.0, 10.0, 0.0);
        let pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase]);
        assert!(
            pipeline.is_active(),
            "pipeline with phases should always report is_active() == true"
        );
    }

    #[test]
    fn backward_compat_no_phases_process() {
        // Explicitly confirm: new() with no phases behaves identically to pre-Phase-3.
        let mut pipeline = FaultPipeline::new(no_fault_profile());
        assert!(
            pipeline.phases.is_empty(),
            "new() should create pipeline with no phases"
        );
        let actions = pipeline.process(b"backward compat packet");
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(&actions[0], FaultAction::Forward { delay, .. } if delay.is_zero()),
            "no-fault pipeline should forward with zero delay"
        );
    }

    #[test]
    fn dynamic_phase_transition() {
        use std::thread;

        // Phase: active immediately, expires after 10ms, drops everything.
        let phase = phase_with_loss(0.0, 0.01, 1.0);
        let mut pipeline = FaultPipeline::with_phases(no_fault_profile(), vec![phase]);

        // Immediately — phase is active, packet should be dropped.
        let actions = pipeline.process(b"during phase");
        assert!(
            actions.iter().all(|a| matches!(a, FaultAction::Drop)),
            "packet during active phase (loss_rate=1.0) should be dropped"
        );

        // Wait for phase to expire.
        thread::sleep(Duration::from_millis(20));

        // Phase expired — default has no faults, packet should be forwarded.
        let actions = pipeline.process(b"after phase");
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, FaultAction::Forward { .. })),
            "packet after phase expiry should be forwarded"
        );
    }

    // --- Phase 5: stats tests ---

    #[test]
    fn stats_initial_all_zero() {
        let pipeline = FaultPipeline::new(no_fault_profile());
        let s = pipeline.stats();
        assert_eq!(s.packets_processed, 0);
        assert_eq!(s.packets_forwarded, 0);
        assert_eq!(s.packets_dropped, 0);
        assert_eq!(s.packets_duplicated, 0);
        assert_eq!(s.packets_replayed, 0);
    }

    #[test]
    fn stats_counts_forward() {
        let mut pipeline = FaultPipeline::new(no_fault_profile());
        for _ in 0..5 {
            pipeline.process(b"pkt");
        }
        let s = pipeline.stats();
        assert_eq!(s.packets_processed, 5);
        assert_eq!(s.packets_forwarded, 5);
        assert_eq!(s.packets_dropped, 0);
    }

    #[test]
    fn stats_counts_drops() {
        let mut profile = no_fault_profile();
        profile.loss_rate = 1.0;
        let mut pipeline = FaultPipeline::new(profile);
        for _ in 0..3 {
            pipeline.process(b"pkt");
        }
        let s = pipeline.stats();
        assert_eq!(s.packets_dropped, 3);
        assert_eq!(s.packets_forwarded, 0);
    }

    #[test]
    fn stats_counts_duplicates() {
        let mut profile = no_fault_profile();
        profile.duplicate_rate = 1.0;
        let mut pipeline = FaultPipeline::new(profile);
        for _ in 0..2 {
            pipeline.process(b"pkt");
        }
        let s = pipeline.stats();
        assert_eq!(s.packets_duplicated, 2);
    }

    #[test]
    fn stats_counts_combined() {
        use std::thread;

        let mut profile = no_fault_profile();
        profile.loss_rate = 0.0;
        profile.duplicate_rate = 1.0;
        profile.replay_stale_ms = 1;
        let mut pipeline = FaultPipeline::new(profile);

        // Seed the replay buffer with a few packets, then wait so they become stale.
        for _ in 0..3 {
            pipeline.process(b"seed");
        }
        thread::sleep(Duration::from_millis(2));

        // Process more packets — these should forward + duplicate, and may also replay.
        for _ in 0..3 {
            pipeline.process(b"pkt");
        }

        let s = pipeline.stats();
        assert!(
            s.packets_forwarded > 0,
            "expected packets_forwarded > 0, got {}",
            s.packets_forwarded
        );
        assert!(
            s.packets_duplicated > 0,
            "expected packets_duplicated > 0, got {}",
            s.packets_duplicated
        );
    }
}
