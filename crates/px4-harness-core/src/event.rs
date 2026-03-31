//! Timestamped event log used to record mission and fault activity during a run.

use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;

/// Types of events that can be recorded.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum EventKind {
    #[serde(rename = "armed")]
    Armed,
    #[serde(rename = "disarmed")]
    Disarmed,
    #[serde(rename = "takeoff")]
    Takeoff { altitude: f64 },
    #[serde(rename = "waypoint_reached")]
    WaypointReached { index: usize, distance_m: f64 },
    #[serde(rename = "land_command")]
    LandCommand,
    #[serde(rename = "landed")]
    Landed,
    #[serde(rename = "packet_dropped")]
    PacketDropped,
    #[serde(rename = "packet_delayed")]
    PacketDelayed { delay_ms: u64 },
    #[serde(rename = "packet_duplicated")]
    PacketDuplicated,
    #[serde(rename = "packet_replayed")]
    PacketReplayed,
    #[serde(rename = "fault_phase_activated")]
    FaultPhaseActivated { after_secs: f64 },
    #[serde(rename = "fault_phase_expired")]
    FaultPhaseExpired { after_secs: f64 },
    #[serde(rename = "info")]
    Info { message: String },
}

/// A single timestamped event.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Seconds since mission start
    pub elapsed_secs: f64,
    /// What happened
    #[serde(flatten)]
    pub kind: EventKind,
}

/// Thread-safe, bounded event log.
///
/// When the buffer is full, the oldest event is discarded (ring buffer behavior).
#[derive(Debug)]
pub struct EventLog {
    events: Mutex<Vec<Event>>,
    start: Instant,
    max_events: usize,
}

impl EventLog {
    /// Create a new event log with the given capacity.
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Mutex::new(Vec::with_capacity(max_events.min(10_000))),
            start: Instant::now(),
            max_events,
        }
    }

    /// Record an event. If the buffer is full, the oldest event is removed.
    pub fn record(&self, kind: EventKind) {
        let elapsed = self.start.elapsed();
        let event = Event {
            elapsed_secs: elapsed.as_secs_f64(),
            kind,
        };
        let mut events = self.events.lock().expect("event log lock poisoned");
        if events.len() >= self.max_events {
            events.remove(0);
        }
        events.push(event);
    }

    /// Get all events as a cloned Vec.
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().expect("event log lock poisoned").clone()
    }

    /// Number of events currently stored.
    pub fn len(&self) -> usize {
        self.events.lock().expect("event log lock poisoned").len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Export all events as pretty-printed JSON.
    pub fn to_json(&self) -> String {
        let events = self.events();
        serde_json::to_string_pretty(&events).expect("failed to serialize event log")
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn new_event_log_is_empty() {
        let log = EventLog::new(100);
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
    }

    #[test]
    fn record_and_retrieve_events() {
        let log = EventLog::new(100);
        log.record(EventKind::Armed);
        log.record(EventKind::Takeoff { altitude: 10.0 });
        log.record(EventKind::Landed);

        assert_eq!(log.len(), 3);
        let events = log.events();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0].kind, EventKind::Armed));
        assert!(matches!(events[1].kind, EventKind::Takeoff { .. }));
        assert!(matches!(events[2].kind, EventKind::Landed));
    }

    #[test]
    fn events_have_increasing_elapsed_time() {
        let log = EventLog::new(100);
        log.record(EventKind::Armed);
        std::thread::sleep(Duration::from_millis(1));
        log.record(EventKind::Takeoff { altitude: 10.0 });
        std::thread::sleep(Duration::from_millis(1));
        log.record(EventKind::Landed);

        let events = log.events();
        assert_eq!(events.len(), 3);
        assert!(
            events[0].elapsed_secs <= events[1].elapsed_secs,
            "elapsed_secs not monotonically increasing: {} > {}",
            events[0].elapsed_secs,
            events[1].elapsed_secs
        );
        assert!(
            events[1].elapsed_secs <= events[2].elapsed_secs,
            "elapsed_secs not monotonically increasing: {} > {}",
            events[1].elapsed_secs,
            events[2].elapsed_secs
        );
    }

    #[test]
    fn bounded_buffer_drops_oldest() {
        let log = EventLog::new(3);
        log.record(EventKind::Armed);
        log.record(EventKind::Takeoff { altitude: 5.0 });
        log.record(EventKind::LandCommand);
        log.record(EventKind::Landed);
        log.record(EventKind::Disarmed);

        assert_eq!(log.len(), 3);
        let events = log.events();
        // The first two (Armed, Takeoff) should be gone; last 3 remain
        assert!(matches!(events[0].kind, EventKind::LandCommand));
        assert!(matches!(events[1].kind, EventKind::Landed));
        assert!(matches!(events[2].kind, EventKind::Disarmed));
    }

    #[test]
    fn to_json_produces_valid_json() {
        let log = EventLog::new(100);
        log.record(EventKind::Armed);
        log.record(EventKind::Landed);

        let json = log.to_json();
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("to_json() must produce valid JSON");
        let arr = value.as_array().expect("JSON output must be an array");
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn to_json_contains_event_type() {
        let log = EventLog::new(100);
        log.record(EventKind::PacketDropped);

        let json = log.to_json();
        assert!(
            json.contains("packet_dropped"),
            "JSON output should contain \"packet_dropped\", got: {json}"
        );
    }

    #[test]
    fn default_capacity() {
        let log = EventLog::default();
        // Capacity is 10_000; recording one event should work without panic
        log.record(EventKind::Info {
            message: "hello".to_string(),
        });
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn concurrent_record_no_panic() {
        const THREADS: usize = 10;
        const EVENTS_PER_THREAD: usize = 100;
        const MAX_EVENTS: usize = 500;

        let log = Arc::new(EventLog::new(MAX_EVENTS));
        let mut handles = Vec::with_capacity(THREADS);

        for _ in 0..THREADS {
            let log_clone = Arc::clone(&log);
            handles.push(std::thread::spawn(move || {
                for i in 0..EVENTS_PER_THREAD {
                    log_clone.record(EventKind::Info {
                        message: format!("event {i}"),
                    });
                }
            }));
        }

        for handle in handles {
            handle.join().expect("thread panicked");
        }

        assert!(
            log.len() <= MAX_EVENTS,
            "len() {} exceeds max_events {}",
            log.len(),
            MAX_EVENTS
        );
    }
}
