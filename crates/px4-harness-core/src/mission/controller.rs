use std::sync::Arc;

use ::mavlink::ardupilotmega::*;
use ::mavlink::MavHeader;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep, timeout};

use crate::error::HarnessError;
use crate::mavlink::connection::MavlinkConnection;

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
    fn send_command(
        &self,
        command: MavCmd,
        params: [f32; 7],
    ) -> Result<(), HarnessError> {
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
        // MAV_CMD_DO_SET_MODE: param1 = base mode (1 = CUSTOM), param2 = custom mode
        // PX4 offboard custom mode = 6 << 16 = 393216
        let custom_mode = (6_u32 << 16) as f32;
        self.send_command(
            MavCmd::MAV_CMD_DO_SET_MODE,
            [1.0, custom_mode, 0.0, 0.0, 0.0, 0.0, 0.0],
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
        let msg = MavMessage::SET_POSITION_TARGET_GLOBAL_INT(
            SET_POSITION_TARGET_GLOBAL_INT_DATA {
                time_boot_ms: 0,
                target_system: self.target_system,
                target_component: self.target_component,
                coordinate_frame: MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT_INT,
                type_mask: PositionTargetTypemask::DEFAULT,
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
            },
        );
        self.connection.send(&msg)
    }

    /// Execute a full mission: set mode, arm, takeoff, fly waypoints, land.
    pub async fn run_mission(
        &self,
        mission: &crate::scenario::Mission,
        mut rx: mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>>,
    ) -> Result<(), HarnessError> {
        let altitude = mission.takeoff_altitude as f32;

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

        // Step 1: Start sending setpoints BEFORE switching to offboard mode.
        // PX4 requires active setpoint stream before it accepts offboard mode.
        println!("Starting setpoint stream...");
        let conn = self.connection.clone();
        let first_wp = &mission.waypoints[0];
        let lat = first_wp.latitude;
        let lon = first_wp.longitude;
        let alt = first_wp.altitude as f32;

        // Spawn a task that sends setpoints at 2Hz
        let setpoint_handle = tokio::spawn({
            let conn = conn.clone();
            async move {
                loop {
                    let _ = conn.send(&MavMessage::SET_POSITION_TARGET_GLOBAL_INT(
                        SET_POSITION_TARGET_GLOBAL_INT_DATA {
                            time_boot_ms: 0,
                            target_system: 1,
                            target_component: 1,
                            coordinate_frame: MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT_INT,
                            type_mask: PositionTargetTypemask::DEFAULT,
                            lat_int: (lat * 1e7) as i32,
                            lon_int: (lon * 1e7) as i32,
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

        // Wait for PX4 to finish pre-flight checks (GPS lock, EKF, etc.)
        self.wait_for_ready(&mut rx).await?;

        // Step 2: Arm first (PX4 may reject arming in offboard mode)
        self.arm(&mut rx).await?;

        // Step 3: Then switch to offboard mode
        self.set_mode_offboard(&mut rx).await?;

        // Step 4: Takeoff
        self.takeoff(altitude, &mut rx).await?;

        // Step 5: Wait for takeoff to complete
        println!("Waiting for takeoff to complete...");
        sleep(Duration::from_secs(10)).await;

        // Step 6: Fly to each waypoint
        for (i, waypoint) in mission.waypoints.iter().enumerate() {
            println!(
                "Flying to waypoint {} ({}, {}, {}m)...",
                i, waypoint.latitude, waypoint.longitude, waypoint.altitude
            );
            self.send_position_setpoint(
                waypoint.latitude,
                waypoint.longitude,
                waypoint.altitude as f32,
            )?;

            // Wait for the drone to reach the waypoint
            // (In Phase 5, we'll check telemetry for actual position)
            sleep(Duration::from_secs(15)).await;
            println!("Waypoint {} reached (assumed)", i);
        }

        // Step 7: Land
        setpoint_handle.abort(); // stop sending setpoints
        heartbeat_handle.abort(); // stop sending heartbeats
        self.land(&mut rx).await?;

        println!("Mission complete");
        Ok(())
    }
}