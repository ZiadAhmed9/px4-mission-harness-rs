use ::mavlink::ardupilotmega::*;
use ::mavlink::MavHeader;
use std::time::Instant;
use tokio::sync::mpsc;

use std::sync::Arc;

use super::store::*;
use crate::error::HarnessError;

pub fn start_telemetry_processor(
    mut rx: mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    store: Arc<TelemetryStore>,
) -> mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>> {
    let (tx, new_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(result) = rx.recv().await {
            if let Ok((ref header, ref msg)) = result {
                process_message(header, msg, &store);
            }
            // Forward the message to the mission controller
            let _ = tx.send(result);
        }
    });

    new_rx
}

fn process_message(header: &MavHeader, msg: &MavMessage, store: &TelemetryStore) {
    // Only process messages from the autopilot (system 1)
    if header.system_id != 1 {
        return;
    }

    match msg {
        MavMessage::GLOBAL_POSITION_INT(pos) => {
            store.record_position(PositionSample {
                timestamp: Instant::now(),
                latitude: pos.lat as f64 / 1e7,
                longitude: pos.lon as f64 / 1e7,
                altitude_msl: pos.alt as f64 / 1000.0,
                relative_alt: pos.relative_alt as f64 / 1000.0,
                vx: pos.vx as f32 / 100.0,
                vy: pos.vy as f32 / 100.0,
                vz: pos.vz as f32 / 100.0,
            });
        }
        MavMessage::ATTITUDE(att) => {
            store.record_attitude(AttitudeSample {
                timestamp: Instant::now(),
                roll: att.roll,
                pitch: att.pitch,
                yaw: att.yaw,
            });
        }
        MavMessage::HEARTBEAT(hb) => {
            store.record_status(VehicleStatus {
                timestamp: Instant::now(),
                armed: hb
                    .base_mode
                    .contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED),
                flight_mode: hb.custom_mode,
                system_status: hb.system_status as u8,
            });
        }
        MavMessage::EXTENDED_SYS_STATE(ext) => {
            let state = match ext.landed_state {
                MavLandedState::MAV_LANDED_STATE_ON_GROUND => LandedState::OnGround,
                MavLandedState::MAV_LANDED_STATE_IN_AIR => LandedState::InAir,
                MavLandedState::MAV_LANDED_STATE_TAKEOFF => LandedState::Takeoff,
                MavLandedState::MAV_LANDED_STATE_LANDING => LandedState::Landing,
                _ => LandedState::Undefined,
            };
            store.update_landed_state(state);
        }
        _ => {} // ignore all other messages
    }
}
