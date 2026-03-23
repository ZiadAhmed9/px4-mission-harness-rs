use std::sync::Arc;

use mavlink::ardupilotmega::MavMessage;
use mavlink::{MavConnection, MavHeader};
use tokio::sync::mpsc;

use crate::error::HarnessError;

/// Wraps a MAVLink UDP connection to PX4 SITL.
pub struct MavlinkConnection {
    connection: Box<dyn MavConnection<MavMessage> + Send + Sync>,
    system_id: u8,
    component_id: u8,
}

impl MavlinkConnection {
    /// Connect to PX4 SITL at the given UDP address.
    /// Example address: "udpin:0.0.0.0:14540"
    pub fn connect(address: &str) -> Result<Self, HarnessError> {
        let connection = mavlink::connect::<MavMessage>(address)
            .map_err(|e| HarnessError::MavlinkConnection {
                address: address.to_string(),
                source: e,
            })?;

        Ok(Self {
            connection,
            system_id: 255,
            component_id: 0,
        })
    }

    /// Blocks until a message arrives.
    pub fn recv(&self) -> Result<(MavHeader, MavMessage), HarnessError> {
        self.connection
            .recv()
            .map_err(|e| HarnessError::MavlinkReceive {
                source: e,
            })
    }

    /// Spawn a background task that receives messages and sends them through a channel.
    /// Takes Arc<Self> so the connection can still be used for sending.
    pub fn start_recv_task(
        self: &Arc<Self>,
    ) -> mpsc::UnboundedReceiver<Result<(MavHeader, MavMessage), HarnessError>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = Arc::clone(self);

        tokio::task::spawn_blocking(move || {
            loop {
                let result = conn.recv();
                if tx.send(result).is_err() {
                    break; // receiver dropped, stop
                }
            }
        });

        rx
    }

    /// Send a MAVLink message to PX4.
    pub fn send(&self, message: &MavMessage) -> Result<(), HarnessError> {
        let header = MavHeader {
            system_id: self.system_id,
            component_id: self.component_id,
            sequence: 0,
        };
        self.connection
            .send(&header, message)
            .map_err(|e| HarnessError::MavlinkSend {
                source: e,
            })?;
        Ok(())
    }
}