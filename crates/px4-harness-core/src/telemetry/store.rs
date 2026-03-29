use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PositionSample {
    pub timestamp: Instant,
    pub latitude: f64,     // degrees
    pub longitude: f64,    // degrees
    pub altitude_msl: f64, // meters above sea level
    pub relative_alt: f64, // meters above home/takeoff point
    pub vx: f32,           // ground speed X (m/s)
    pub vy: f32,           // ground speed Y (m/s)
    pub vz: f32,           // vertical speed (m/s, positive = up)
}

/// A single attitude reading (orientation).
#[derive(Debug, Clone)]
pub struct AttitudeSample {
    pub timestamp: Instant,
    pub roll: f32,  // radians
    pub pitch: f32, // radians
    pub yaw: f32,   // radians
}

/// Vehicle state snapshot from heartbeat.
#[derive(Debug, Clone)]
pub struct VehicleStatus {
    pub timestamp: Instant,
    pub armed: bool,
    pub flight_mode: u32,  // PX4 custom_mode bitfield
    pub system_status: u8, // MAV_STATE enum value
}

/// Landed state from EXTENDED_SYS_STATE.
#[derive(Debug, Clone, PartialEq)]
pub enum LandedState {
    Undefined,
    OnGround,
    InAir,
    Takeoff,
    Landing,
}
/// Shared between the receiver task (writes) and assertion engine (reads).
#[derive(Debug)]
pub struct TelemetryStore {
    pub positions: Mutex<Vec<PositionSample>>,
    pub attitudes: Mutex<Vec<AttitudeSample>>,
    pub statuses: Mutex<Vec<VehicleStatus>>,
    pub landed_state: Mutex<LandedState>,
    pub mission_start: Instant,
}

impl Default for TelemetryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryStore {
    pub fn new() -> Self {
        Self {
            positions: Mutex::new(Vec::new()),
            attitudes: Mutex::new(Vec::new()),
            statuses: Mutex::new(Vec::new()),
            landed_state: Mutex::new(LandedState::Undefined),
            mission_start: Instant::now(),
        }
    }

    pub fn record_position(&self, sample: PositionSample) {
        self.positions.lock().unwrap().push(sample);
    }

    pub fn record_attitude(&self, sample: AttitudeSample) {
        self.attitudes.lock().unwrap().push(sample);
    }

    pub fn record_status(&self, status: VehicleStatus) {
        self.statuses.lock().unwrap().push(status);
    }

    pub fn update_landed_state(&self, state: LandedState) {
        *self.landed_state.lock().unwrap() = state;
    }

    /// Get the latest position, if any.
    pub fn latest_position(&self) -> Option<PositionSample> {
        self.positions.lock().unwrap().last().cloned()
    }

    /// Get the current landed state.
    pub fn current_landed_state(&self) -> LandedState {
        self.landed_state.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    fn make_position(lat: f64, lon: f64) -> PositionSample {
        PositionSample {
            timestamp: Instant::now(),
            latitude: lat,
            longitude: lon,
            altitude_msl: 10.0,
            relative_alt: 10.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        }
    }

    #[test]
    fn new_initializes_empty() {
        let store = TelemetryStore::new();
        assert!(store.positions.lock().unwrap().is_empty());
        assert!(store.attitudes.lock().unwrap().is_empty());
        assert!(store.statuses.lock().unwrap().is_empty());
        assert_eq!(store.current_landed_state(), LandedState::Undefined);
    }

    #[test]
    fn record_position_then_latest() {
        let store = TelemetryStore::new();
        store.record_position(make_position(47.3977, 8.5456));
        let pos = store.latest_position().expect("should have a position");
        assert_eq!(pos.latitude, 47.3977);
        assert_eq!(pos.longitude, 8.5456);
    }

    #[test]
    fn latest_position_empty_store() {
        let store = TelemetryStore::new();
        assert!(store.latest_position().is_none());
    }

    #[test]
    fn record_status_preserves_order() {
        let store = TelemetryStore::new();
        let ts = Instant::now();
        let armed_values = [true, false, true];
        for &armed in &armed_values {
            store.record_status(VehicleStatus {
                timestamp: ts,
                armed,
                flight_mode: 0,
                system_status: 0,
            });
        }
        let statuses = store.statuses.lock().unwrap();
        assert_eq!(statuses.len(), 3);
        assert!(statuses[0].armed);
        assert!(!statuses[1].armed);
        assert!(statuses[2].armed);
    }

    #[test]
    fn update_landed_state_overwrites() {
        let store = TelemetryStore::new();
        store.update_landed_state(LandedState::OnGround);
        store.update_landed_state(LandedState::InAir);
        assert_eq!(store.current_landed_state(), LandedState::InAir);
    }

    #[test]
    fn record_attitude_stores_values() {
        let store = TelemetryStore::new();
        store.record_attitude(AttitudeSample {
            timestamp: Instant::now(),
            roll: 0.1,
            pitch: 0.2,
            yaw: 0.3,
        });
        let attitudes = store.attitudes.lock().unwrap();
        assert_eq!(attitudes.len(), 1);
        assert!((attitudes[0].roll - 0.1).abs() < f32::EPSILON);
        assert!((attitudes[0].pitch - 0.2).abs() < f32::EPSILON);
        assert!((attitudes[0].yaw - 0.3).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn concurrent_position_writes() {
        let store = Arc::new(TelemetryStore::new());
        let mut handles = Vec::new();
        for task_id in 0u64..10 {
            let store_clone = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                for i in 0u64..100 {
                    let lat = (task_id * 100 + i) as f64;
                    store_clone.record_position(make_position(lat, 0.0));
                }
            }));
        }
        for handle in handles {
            handle.await.expect("task should not panic");
        }
        let positions = store.positions.lock().unwrap();
        assert_eq!(positions.len(), 1000);
    }

    #[tokio::test]
    async fn concurrent_read_write_no_panic() {
        let store = Arc::new(TelemetryStore::new());

        let writer_store = Arc::clone(&store);
        let writer = tokio::spawn(async move {
            for i in 0..500 {
                writer_store.record_position(make_position(i as f64, 0.0));
            }
        });

        let reader_store = Arc::clone(&store);
        let reader = tokio::spawn(async move {
            for _ in 0..500 {
                let _ = reader_store.latest_position();
            }
        });

        writer.await.expect("writer should not panic");
        reader.await.expect("reader should not panic");
    }
}
