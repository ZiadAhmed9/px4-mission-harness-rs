use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use ::mavlink::ardupilotmega::*;
use ::mavlink::MavHeader;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration};

use crate::error::HarnessError;
use crate::mavlink::connection::MavlinkConnection;

use crate::telemetry::processor::start_telemetry_processor;
use crate::telemetry::store::{LandedState, TelemetryStore};

pub struct MissionController {
    connection: Arc<MavlinkConnection>,
    target_system: u8,
    target_component: u8,
}

impl MissionController {
    pub fn new(connection: Arc<MavlinkConnection>) -> Self {
        Self {
            connection,
            target_system: 1,
            target_component: 1,
        }
    }

    /// Send a COMMAND_LONG and return immediately (no ACK wait).
    fn send_command(&self, command: MavCmd, params: [f32; 7]) -> Result<(), HarnessError> {
        let msg = MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
            target_system: self.target_system,
            target_component: self.target_component,
            command,
            confirmation: 0,
            param1: params[0],
            param2: params[1],
            param3: params[2],
            param4: params[3],
            param5: params[4],
            param6: params[5],
            param7: params[6],
        });
        self.connection.send(&msg)
    }

    /// Wait for a COMMAND_ACK matching the given command, with a timeout.
    async fn wait_for_ack(
        &self,
        command: MavCmd,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
        timeout_duration: Duration,
    ) -> Result<MavResult, HarnessError> {
        let deadline = timeout(timeout_duration, async {
            while let Some(result) = rx.recv().await {
                match result {
                    Ok((_, MavMessage::COMMAND_ACK(ack))) => {
                        if ack.command == command {
                            return Ok(ack.result);
                        }
                        // ACK for a different command, keep waiting
                    }
                    Ok(_) => {
                        // Not an ACK, ignore (telemetry, heartbeats, etc.)
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(HarnessError::MissionError {
                reason: "message channel closed".to_string(),
            })
        });

        deadline.await.map_err(|_| HarnessError::MissionTimeout {
            command: format!("{:?}", command),
        })?
    }

    /// Wait until PX4 reports it's ready to arm (MAV_STATE_STANDBY in heartbeat).
    async fn wait_for_ready(
        &self,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        println!("Waiting for PX4 to be ready (MAV_STATE_STANDBY)...");
        let ready = timeout(Duration::from_secs(30), async {
            while let Some(result) = rx.recv().await {
                match result {
                    Ok((_, MavMessage::HEARTBEAT(hb))) => {
                        // MAV_STATE_STANDBY = 3, means pre-flight checks passed, ready to arm
                        if hb.system_status == MavState::MAV_STATE_STANDBY {
                            println!("PX4 is ready (MAV_STATE_STANDBY)");
                            return Ok(());
                        }
                        println!(
                            "  PX4 state: {:?} — waiting for STANDBY...",
                            hb.system_status
                        );
                    }
                    Ok(_) => {} // ignore non-heartbeat messages
                    Err(e) => return Err(e),
                }
            }
            Err(HarnessError::MissionError {
                reason: "message channel closed while waiting for ready".to_string(),
            })
        });

        ready.await.map_err(|_| HarnessError::MissionTimeout {
            command: "wait_for_ready".to_string(),
        })?
    }

    /// Arm the vehicle motors.
    pub async fn arm(
        &self,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        println!("Arming...");
        // MAV_CMD_COMPONENT_ARM_DISARM: param1 = 1.0 means arm, 0.0 means disarm
        self.send_command(
            MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )?;

        let result = self
            .wait_for_ack(
                MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
                rx,
                Duration::from_secs(5),
            )
            .await?;

        if result == MavResult::MAV_RESULT_ACCEPTED {
            println!("Armed successfully");
            Ok(())
        } else {
            Err(HarnessError::MissionError {
                reason: format!("arm rejected: {:?}", result),
            })
        }
    }

    /// Command takeoff to the specified altitude.
    pub async fn takeoff(
        &self,
        altitude: f32,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        println!("Taking off to {}m...", altitude);
        // MAV_CMD_NAV_TAKEOFF: param7 = altitude (meters above home)
        self.send_command(
            MavCmd::MAV_CMD_NAV_TAKEOFF,
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, altitude],
        )?;

        let result = self
            .wait_for_ack(MavCmd::MAV_CMD_NAV_TAKEOFF, rx, Duration::from_secs(10))
            .await?;

        if result == MavResult::MAV_RESULT_ACCEPTED {
            println!("Takeoff command accepted");
            Ok(())
        } else {
            Err(HarnessError::MissionError {
                reason: format!("takeoff rejected: {:?}", result),
            })
        }
    }

    /// Command the vehicle to land at current position.
    pub async fn land(
        &self,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        println!("Landing...");
        // MAV_CMD_NAV_LAND: all params 0 = land at current position
        self.send_command(
            MavCmd::MAV_CMD_NAV_LAND,
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )?;

        let result = self
            .wait_for_ack(MavCmd::MAV_CMD_NAV_LAND, rx, Duration::from_secs(10))
            .await?;

        if result == MavResult::MAV_RESULT_ACCEPTED {
            println!("Land command accepted");
            Ok(())
        } else {
            Err(HarnessError::MissionError {
                reason: format!("land rejected: {:?}", result),
            })
        }
    }

    pub async fn set_mode_offboard(
        &self,
        rx: &mut mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        println!("Setting offboard mode...");
        // PX4 uses COMMAND_LONG with MAV_CMD_DO_SET_MODE.
        // param1 = base_mode with MAV_MODE_FLAG_CUSTOM_MODE_ENABLED (1)
        // param2 = PX4 custom main mode (6 = OFFBOARD)
        // param3 = PX4 custom sub mode (0)
        // Note: param2 is the main mode NUMBER (6), NOT shifted.
        // PX4's commander handles the shifting internally for MAV_CMD_DO_SET_MODE.
        self.send_command(
            MavCmd::MAV_CMD_DO_SET_MODE,
            [1.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        )?;

        let result = self
            .wait_for_ack(MavCmd::MAV_CMD_DO_SET_MODE, rx, Duration::from_secs(5))
            .await?;

        if result == MavResult::MAV_RESULT_ACCEPTED {
            println!("Offboard mode set");
            Ok(())
        } else {
            Err(HarnessError::MissionError {
                reason: format!("set mode offboard rejected: {:?}", result),
            })
        }
    }

    /// Send a position setpoint (GPS coordinates).
    /// Call this repeatedly (e.g., every 500ms) to keep PX4 in offboard mode.
    pub fn send_position_setpoint(
        &self,
        latitude: f64,
        longitude: f64,
        altitude: f32,
    ) -> Result<(), HarnessError> {
        let msg = MavMessage::SET_POSITION_TARGET_GLOBAL_INT(SET_POSITION_TARGET_GLOBAL_INT_DATA {
            time_boot_ms: 0,
            target_system: self.target_system,
            target_component: self.target_component,
            coordinate_frame: MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT_INT,
            type_mask: PositionTargetTypemask::from_bits_truncate(0b0000_0111_1111_1000),
            lat_int: (latitude * 1e7) as i32,
            lon_int: (longitude * 1e7) as i32,
            alt: altitude,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            afx: 0.0,
            afy: 0.0,
            afz: 0.0,
            yaw: 0.0,
            yaw_rate: 0.0,
        });
        self.connection.send(&msg)
    }

    /// Execute a full mission: set mode, arm, takeoff, fly waypoints, land.
    pub async fn run_mission(
        &self,
        mission: &crate::scenario::Mission,
        rx: mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
        verbose: bool,
    ) -> Result<Arc<TelemetryStore>, HarnessError> {
        let store = Arc::new(TelemetryStore::new());
        let altitude = mission.takeoff_altitude as f32;

        // Insert telemetry processor between receiver and controller
        let mut rx = start_telemetry_processor(rx, Arc::clone(&store));

        // Optional: periodic telemetry logger (only when --verbose)
        let debug_handle = if verbose {
            let debug_store = Arc::clone(&store);
            Some(tokio::spawn(async move {
                loop {
                    if let Some(pos) = debug_store.latest_position() {
                        println!(
                            "  [TELEM] pos=({:.6}, {:.6}) alt={:.1}m",
                            pos.latitude, pos.longitude, pos.relative_alt
                        );
                    }
                    sleep(Duration::from_secs(3)).await;
                }
            }))
        } else {
            None
        };

        // Step 0: Send GCS heartbeats so PX4 knows we're connected.
        // Without this, PX4 refuses to arm ("No connection to the GCS").
        let heartbeat_handle = tokio::spawn({
            let conn = self.connection.clone();
            async move {
                loop {
                    let _ = conn.send(&MavMessage::HEARTBEAT(HEARTBEAT_DATA {
                        custom_mode: 0,
                        mavtype: MavType::MAV_TYPE_GCS,
                        autopilot: MavAutopilot::MAV_AUTOPILOT_INVALID,
                        base_mode: MavModeFlag::empty(),
                        system_status: MavState::MAV_STATE_ACTIVE,
                        mavlink_version: 0x3,
                    }));
                    sleep(Duration::from_secs(1)).await;
                }
            }
        });
        println!("Sending GCS heartbeats...");

        // Step 1: Wait for PX4 to be ready and get current position for initial setpoint
        self.wait_for_ready(&mut rx).await?;

        // Get the drone's current position to use as initial setpoint
        // (we hold position until we're ready to move)
        let initial_pos = loop {
            if let Some(pos) = store.latest_position() {
                break pos;
            }
            sleep(Duration::from_millis(100)).await;
        };

        // Shared target position — the setpoint task reads these atomically,
        // and we update them as the mission progresses.
        // Stored as degE7 (i32) and mm (i32) to use AtomicI32.
        let target_lat = Arc::new(AtomicI32::new((initial_pos.latitude * 1e7) as i32));
        let target_lon = Arc::new(AtomicI32::new((initial_pos.longitude * 1e7) as i32));
        let target_alt = Arc::new(AtomicI32::new(0)); // 0m relative alt — stay on ground initially

        // Start sending setpoints at 2Hz BEFORE switching to offboard mode.
        // PX4 requires an active setpoint stream before it accepts offboard mode.
        println!("Starting setpoint stream...");
        let setpoint_handle = tokio::spawn({
            let conn = self.connection.clone();
            let t_lat = Arc::clone(&target_lat);
            let t_lon = Arc::clone(&target_lon);
            let t_alt = Arc::clone(&target_alt);
            async move {
                loop {
                    let lat = t_lat.load(Ordering::Relaxed);
                    let lon = t_lon.load(Ordering::Relaxed);
                    let alt = t_alt.load(Ordering::Relaxed) as f32 / 1000.0; // mm -> m
                    let _ = conn.send(&MavMessage::SET_POSITION_TARGET_GLOBAL_INT(
                        SET_POSITION_TARGET_GLOBAL_INT_DATA {
                            time_boot_ms: 0,
                            target_system: 1,
                            target_component: 1,
                            coordinate_frame: MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT_INT,
                            // type_mask: bits that are SET mean IGNORE that field.
                            // We want PX4 to use lat, lon, alt (bits 0,1,2 = 0)
                            // and ignore vx,vy,vz,ax,ay,az,yaw,yaw_rate (bits 3-10 = 1)
                            type_mask: PositionTargetTypemask::from_bits_truncate(
                                0b0000_0111_1111_1000
                            ),
                            lat_int: lat,
                            lon_int: lon,
                            alt,
                            vx: 0.0, vy: 0.0, vz: 0.0,
                            afx: 0.0, afy: 0.0, afz: 0.0,
                            yaw: 0.0, yaw_rate: 0.0,
                        },
                    ));
                    sleep(Duration::from_millis(500)).await;
                }
            }
        });

        // Give PX4 a moment to see the setpoint stream
        sleep(Duration::from_secs(2)).await;

        // Step 2: Switch to offboard mode FIRST (while disarmed, setpoints already streaming)
        self.set_mode_offboard(&mut rx).await?;

        // Step 3: Then arm (PX4 accepts arming in offboard mode)
        self.arm(&mut rx).await?;

        // Step 4: Takeoff — in offboard mode, we just set the target altitude.
        // MAV_CMD_NAV_TAKEOFF doesn't work in offboard mode.
        println!("Taking off to {}m...", altitude);
        target_alt.store((altitude * 1000.0) as i32, Ordering::Relaxed);

        // Wait until telemetry confirms we're near the target altitude
        let takeoff_ok = timeout(Duration::from_secs(30), async {
            loop {
                if let Some(pos) = store.latest_position() {
                    if pos.relative_alt > (altitude as f64 - 1.0) {
                        return;
                    }
                }
                sleep(Duration::from_millis(500)).await;
            }
        }).await;

        match takeoff_ok {
            Ok(_) => println!("Takeoff complete (verified by telemetry)"),
            Err(_) => println!("Takeoff altitude not reached within timeout"),
        }

        // Step 6: Fly to each waypoint
        for (i, waypoint) in mission.waypoints.iter().enumerate() {
            println!(
                "Flying to waypoint {} ({}, {}, {}m)...",
                i, waypoint.latitude, waypoint.longitude, waypoint.altitude
            );
            // Update the shared target — the setpoint task picks this up automatically
            target_lat.store((waypoint.latitude * 1e7) as i32, Ordering::Relaxed);
            target_lon.store((waypoint.longitude * 1e7) as i32, Ordering::Relaxed);
            target_alt.store((waypoint.altitude * 1000.0) as i32, Ordering::Relaxed);

            // Wait for the drone to reach the waypoint
            // Wait until telemetry shows we're within acceptance_radius of the waypoint
            let accepted = timeout(Duration::from_secs(60), async {
                loop {
                    if let Some(pos) = store.latest_position() {
                        let distance = Self::haversine_distance(
                            pos.latitude,
                            pos.longitude,
                            waypoint.latitude,
                            waypoint.longitude,
                        );
                        if distance < waypoint.acceptance_radius {
                            return;
                        }
                    }
                    sleep(Duration::from_millis(500)).await;
                }
            })
            .await;

            match accepted {
                Ok(_) => println!("Waypoint {} reached (verified by telemetry)", i),
                Err(_) => println!("Waypoint {} not reached within timeout", i),
            }
        }

        // Step 7: Land
        setpoint_handle.abort(); // stop sending setpoints
        heartbeat_handle.abort(); // stop sending heartbeats
        self.land(&mut rx).await?;

        // Step 8: Verify landing with telemetry
        println!("Waiting for landing...");
        let landed = timeout(Duration::from_secs(60), async {
            loop {
                if store.current_landed_state() == LandedState::OnGround {
                    return;
                }
                sleep(Duration::from_millis(500)).await;
            }
        })
        .await;

        match landed {
            Ok(_) => println!("Landing confirmed by telemetry"),
            Err(_) => println!("Landing not confirmed within timeout"),
        }

        // Wait a few seconds for PX4 to disarm and send the disarmed heartbeat.
        // This ensures the assertion engine can detect the armed->disarmed transition.
        sleep(Duration::from_secs(5)).await;

        println!("Mission complete");

        if let Some(h) = debug_handle {
            h.abort();
        }
        Ok(store) // Return the telemetry store for assertions after the mission
    }

    pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        const R: f64 = 6_371_000.0; // Earth's radius in meters

        let d_lat = (lat2 - lat1).to_radians();
        let d_lon = (lon2 - lon1).to_radians();

        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);

        let c = 2.0 * a.sqrt().asin();
        R * c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_same_point() {
        let d = MissionController::haversine_distance(47.397742, 8.545594, 47.397742, 8.545594);
        assert!(d < 0.01, "same point should be ~0m, got {}", d);
    }

    #[test]
    fn haversine_known_distance() {
        // Two points ~111m apart (0.001 degrees of latitude)
        let d = MissionController::haversine_distance(47.397742, 8.545594, 47.398742, 8.545594);
        assert!(
            (d - 111.2).abs() < 1.0,
            "expected ~111m, got {}",
            d
        );
    }

    #[test]
    fn haversine_short_distance() {
        // ~5m apart — typical acceptance radius check
        let d = MissionController::haversine_distance(47.397742, 8.545594, 47.397787, 8.545594);
        assert!(d > 3.0 && d < 7.0, "expected ~5m, got {}", d);
    }
}
