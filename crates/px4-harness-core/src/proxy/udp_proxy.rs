use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::UdpSocket;
use tokio::time::sleep;

use crate::fault::pipeline::{FaultAction, FaultPipeline};
use crate::scenario::FaultProfile;

/// A UDP proxy that sits between PX4 SITL and a client (our harness or any GCS).
///
/// Uses two sockets:
///   - px4_socket: binds to `px4_listen_port` where PX4 sends its MAVLink data.
///     PX4's source address is learned from the first packet it sends.
///   - client_socket: listens on `client_listen_port` for the harness/GCS to connect.
///
/// Architecture:
///   PX4 (sends to px4_listen_port) ←→ px4_socket ←→ [FAULTS] ←→ client_socket ←→ Client
pub struct UdpProxy {
    /// Port where PX4 sends its MAVLink output (e.g., 14550 for GCS channel)
    px4_listen_port: u16,
    /// Port where our harness/GCS connects (e.g., 14560)
    client_listen_port: u16,
}

impl UdpProxy {
    pub fn new(px4_listen_port: u16, client_listen_port: u16) -> Self {
        Self {
            px4_listen_port,
            client_listen_port,
        }
    }

    /// Start the proxy. Returns the client-facing listen address and a handle to stop it.
    pub async fn start(
        &self,
        fault_profile: FaultProfile,
    ) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
        // Socket facing PX4 — bind to the port PX4 sends to
        let px4_listen: SocketAddr = format!("0.0.0.0:{}", self.px4_listen_port).parse().unwrap();
        let px4_socket = Arc::new(UdpSocket::bind(px4_listen).await?);
        // PX4's source address is learned from the first packet it sends
        let px4_addr_store: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));

        // Socket facing the client (our harness)
        let client_listen: SocketAddr =
            format!("0.0.0.0:{}", self.client_listen_port).parse().unwrap();
        let client_socket = Arc::new(UdpSocket::bind(client_listen).await?);
        let client_addr_store: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));

        let proxy_addr = client_socket.local_addr()?;

        // Fault pipelines — one per direction
        let px4_to_client = Arc::new(Mutex::new(FaultPipeline::new(fault_profile.clone())));
        let client_to_px4 = Arc::new(Mutex::new(FaultPipeline::new(fault_profile)));

        // Task 1: PX4 → Client (receive from PX4, apply faults, forward to client)
        let handle1 = {
            let px4_sock = Arc::clone(&px4_socket);
            let cli_sock = Arc::clone(&client_socket);
            let pipeline = Arc::clone(&px4_to_client);
            let client_addr = Arc::clone(&client_addr_store);
            let px4_addr = Arc::clone(&px4_addr_store);

            tokio::spawn(async move {
                let mut buf = [0u8; 65535];
                loop {
                    let (len, src_addr) = match px4_sock.recv_from(&mut buf).await {
                        Ok(result) => result,
                        Err(_) => continue,
                    };
                    let packet = &buf[..len];

                    // Learn PX4's source address from the first packet
                    { *px4_addr.lock().unwrap() = Some(src_addr); }

                    // Only forward if we know the client's address
                    let target = { *client_addr.lock().unwrap() };
                    if let Some(client) = target {
                        let actions = pipeline.lock().unwrap().process(packet);
                        apply_actions(actions, client, &cli_sock);
                    }
                }
            })
        };

        // Task 2: Client → PX4 (receive from client, apply faults, forward to PX4)
        let handle2 = {
            let px4_sock = Arc::clone(&px4_socket);
            let cli_sock = Arc::clone(&client_socket);
            let pipeline = Arc::clone(&client_to_px4);
            let client_addr = Arc::clone(&client_addr_store);
            let px4_addr = Arc::clone(&px4_addr_store);

            tokio::spawn(async move {
                let mut buf = [0u8; 65535];
                loop {
                    let (len, src_addr) = match cli_sock.recv_from(&mut buf).await {
                        Ok(result) => result,
                        Err(_) => continue,
                    };
                    let packet = &buf[..len];

                    // Remember the client's address
                    { *client_addr.lock().unwrap() = Some(src_addr); }

                    // Forward to PX4 (if we know its address)
                    let target = { *px4_addr.lock().unwrap() };
                    if let Some(px4) = target {
                        let actions = pipeline.lock().unwrap().process(packet);
                        apply_actions(actions, px4, &px4_sock);
                    }
                }
            })
        };

        // Wrap both tasks into one handle
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = handle1 => {}
                _ = handle2 => {}
            }
        });

        Ok((proxy_addr, handle))
    }
}

/// Send packets according to fault actions — immediately or after a delay.
fn apply_actions(actions: Vec<FaultAction>, target: SocketAddr, socket: &Arc<UdpSocket>) {
    for action in actions {
        match action {
            FaultAction::Forward { data, delay } => {
                let sock = Arc::clone(socket);
                if delay.is_zero() {
                    tokio::spawn(async move {
                        let _ = sock.send_to(&data, target).await;
                    });
                } else {
                    tokio::spawn(async move {
                        sleep(delay).await;
                        let _ = sock.send_to(&data, target).await;
                    });
                }
            }
            FaultAction::Drop => {} // silently drop
        }
    }
}
