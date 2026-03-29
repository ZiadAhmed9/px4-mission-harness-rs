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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(system_id: u8) -> MavHeader {
        MavHeader {
            system_id,
            component_id: 0,
            sequence: 0,
        }
    }

    #[test]
    fn global_position_int_converts_correctly() {
        let store = TelemetryStore::new();
        let header = make_header(1);
        let msg = MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA {
            time_boot_ms: 0,
            lat: 473_977_420,
            lon: 85_455_940,
            alt: 10_000,
            relative_alt: 5_000,
            vx: 100,
            vy: 200,
            vz: -50,
            hdg: 0,
        });

        process_message(&header, &msg, &store);

        let positions = store.positions.lock().unwrap();
        assert_eq!(positions.len(), 1);
        let pos = &positions[0];
        assert!((pos.latitude - 47.397_742).abs() < 1e-6);
        assert!((pos.longitude - 8.545_594).abs() < 1e-6);
        assert!((pos.altitude_msl - 10.0).abs() < 1e-6);
        assert!((pos.relative_alt - 5.0).abs() < 1e-6);
        assert!((pos.vx - 1.0).abs() < 1e-6);
        assert!((pos.vy - 2.0).abs() < 1e-6);
        assert!((pos.vz - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn heartbeat_extracts_armed_flag() {
        let store = TelemetryStore::new();
        let header = make_header(1);
        let msg = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: 0,
            mavtype: MavType::MAV_TYPE_GCS,
            autopilot: MavAutopilot::MAV_AUTOPILOT_INVALID,
            base_mode: MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED,
            system_status: MavState::MAV_STATE_ACTIVE,
            mavlink_version: 3,
        });

        process_message(&header, &msg, &store);

        let statuses = store.statuses.lock().unwrap();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].armed);
    }

    #[test]
    fn heartbeat_unarmed() {
        let store = TelemetryStore::new();
        let header = make_header(1);
        let msg = MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: 0,
            mavtype: MavType::MAV_TYPE_GCS,
            autopilot: MavAutopilot::MAV_AUTOPILOT_INVALID,
            base_mode: MavModeFlag::empty(),
            system_status: MavState::MAV_STATE_STANDBY,
            mavlink_version: 3,
        });

        process_message(&header, &msg, &store);

        let statuses = store.statuses.lock().unwrap();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].armed);
    }

    #[test]
    fn extended_sys_state_maps_landed_states() {
        let cases: &[(MavLandedState, LandedState)] = &[
            (
                MavLandedState::MAV_LANDED_STATE_ON_GROUND,
                LandedState::OnGround,
            ),
            (MavLandedState::MAV_LANDED_STATE_IN_AIR, LandedState::InAir),
            (
                MavLandedState::MAV_LANDED_STATE_TAKEOFF,
                LandedState::Takeoff,
            ),
            (
                MavLandedState::MAV_LANDED_STATE_LANDING,
                LandedState::Landing,
            ),
        ];

        for (mav_state, expected) in cases {
            let store = TelemetryStore::new();
            let header = make_header(1);
            let msg = MavMessage::EXTENDED_SYS_STATE(EXTENDED_SYS_STATE_DATA {
                vtol_state: MavVtolState::MAV_VTOL_STATE_UNDEFINED,
                landed_state: *mav_state,
            });

            process_message(&header, &msg, &store);

            assert_eq!(
                store.current_landed_state(),
                *expected,
                "failed for mav_state {:?}",
                mav_state
            );
        }
    }

    #[test]
    fn non_autopilot_system_id_ignored() {
        let store = TelemetryStore::new();
        let header = make_header(2);
        let msg = MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA {
            time_boot_ms: 0,
            lat: 473_977_420,
            lon: 85_455_940,
            alt: 10_000,
            relative_alt: 5_000,
            vx: 100,
            vy: 200,
            vz: -50,
            hdg: 0,
        });

        process_message(&header, &msg, &store);

        assert!(store.positions.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_message_type_not_stored() {
        let store = TelemetryStore::new();
        let header = make_header(1);
        let msg = MavMessage::PARAM_VALUE(PARAM_VALUE_DATA {
            param_value: 1.0,
            param_count: 1,
            param_index: 0,
            param_id: Default::default(),
            param_type: MavParamType::MAV_PARAM_TYPE_REAL32,
        });

        process_message(&header, &msg, &store);

        assert!(store.positions.lock().unwrap().is_empty());
        assert!(store.attitudes.lock().unwrap().is_empty());
        assert!(store.statuses.lock().unwrap().is_empty());
    }
}
