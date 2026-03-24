use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rand::Rng;

use crate::scenario::FaultProfile;

/// What to do with a packet after passing through the fault pipeline.
pub enum FaultAction {
    /// Forward the packet after the specified delay
    Forward { data: Vec<u8>, delay: Duration },
    /// Drop the packet entirely
    Drop,
}

/// Processes raw UDP packets through a series of fault stages:
/// Drop → Delay/Jitter → Duplicate → Replay
pub struct FaultPipeline {
    profile: FaultProfile,
    /// Recent packets stored for potential stale replay
    replay_buffer: VecDeque<(Instant, Vec<u8>)>,
    /// When > 0, drop packets unconditionally (burst loss)
    burst_remaining: u32,
}

impl FaultPipeline {
    pub fn new(profile: FaultProfile) -> Self {
        Self {
            profile,
            replay_buffer: VecDeque::with_capacity(100),
            burst_remaining: 0,
        }
    }

    /// Process a packet through the fault pipeline.
    /// Returns zero or more actions — a single packet may produce:
    /// - 0 actions (dropped)
    /// - 1 action (normal forward)
    /// - 2+ actions (duplicated and/or replayed)
    pub fn process(&mut self, data: &[u8]) -> Vec<FaultAction> {
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
            return vec![FaultAction::Drop];
        }

        // Stage 2: Random drop
        if self.profile.loss_rate > 0.0 && rng.random::<f64>() < self.profile.loss_rate {
            // Start a burst if configured
            if self.profile.burst_loss_length > 1 {
                self.burst_remaining = self.profile.burst_loss_length - 1;
            }
            return vec![FaultAction::Drop];
        }

        // Stage 3: Calculate delay (fixed + random jitter)
        let base_delay = Duration::from_millis(self.profile.delay_ms);
        let jitter = if self.profile.jitter_ms > 0 {
            Duration::from_millis(rng.random_range(0..=self.profile.jitter_ms))
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

        // Stage 5: Duplicate check — send the packet a second time
        if self.profile.duplicate_rate > 0.0 && rng.random::<f64>() < self.profile.duplicate_rate {
            let dup_jitter = if self.profile.jitter_ms > 0 {
                Duration::from_millis(rng.random_range(0..=self.profile.jitter_ms))
            } else {
                Duration::ZERO
            };
            actions.push(FaultAction::Forward {
                data: data.to_vec(),
                delay: total_delay + dup_jitter,
            });
        }

        // Stage 6: Replay stale — inject an old packet from N ms ago
        if self.profile.replay_stale_ms > 0 {
            let stale_threshold = Duration::from_millis(self.profile.replay_stale_ms);
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
            }
        }

        actions
    }

    /// Returns true if any fault is enabled in the profile.
    pub fn is_active(&self) -> bool {
        self.profile.delay_ms > 0
            || self.profile.jitter_ms > 0
            || self.profile.loss_rate > 0.0
            || self.profile.burst_loss_length > 0
            || self.profile.duplicate_rate > 0.0
            || self.profile.replay_stale_ms > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::FaultProfile;

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
        assert!(
            matches!(&actions[0], FaultAction::Forward { delay, .. } if delay.is_zero())
        );
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
}
