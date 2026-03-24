use std::time::Instant;
use std::sync::Mutex;



#[derive(Debug, Clone)]
pub struct PositionSample {
    pub timestamp: Instant,
    pub latitude: f64,      // degrees
    pub longitude: f64,     // degrees
    pub altitude_msl: f64,  // meters above sea level
    pub relative_alt: f64,  // meters above home/takeoff point
    pub vx: f32,            // ground speed X (m/s)
    pub vy: f32,            // ground speed Y (m/s)
    pub vz: f32,            // vertical speed (m/s, positive = up)
}

/// A single attitude reading (orientation).
#[derive(Debug, Clone)]
pub struct AttitudeSample {
    pub timestamp: Instant,
    pub roll: f32,          // radians
    pub pitch: f32,         // radians
    pub yaw: f32,           // radians
}


/// Vehicle state snapshot from heartbeat.
#[derive(Debug, Clone)]
pub struct VehicleStatus {
    pub timestamp: Instant,
    pub armed: bool,
    pub flight_mode: u32,   // PX4 custom_mode bitfield
    pub system_status: u8,  // MAV_STATE enum value
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