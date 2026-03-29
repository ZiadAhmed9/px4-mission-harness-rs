use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::UdpSocket;
use tokio::time::sleep;

use crate::fault::pipeline::{FaultAction, FaultPipeline};
use crate::scenario::{FaultPhase, FaultProfile};

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
    ///
    /// `fault_phases` is optional: pass an empty `Vec` for static-profile-only behavior
    /// (identical to the pre-Phase-3 behavior).
    pub async fn start(
        &self,
        fault_profile: FaultProfile,
        fault_phases: Vec<FaultPhase>,
    ) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
        // Socket facing PX4 — bind to the port PX4 sends to
        let px4_listen: SocketAddr = format!("0.0.0.0:{}", self.px4_listen_port).parse().unwrap();
        let px4_socket = Arc::new(UdpSocket::bind(px4_listen).await?);
        // PX4's source address is learned from the first packet it sends
        let px4_addr_store: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));

        // Socket facing the client (our harness)
        let client_listen: SocketAddr = format!("0.0.0.0:{}", self.client_listen_port)
            .parse()
            .unwrap();
        let client_socket = Arc::new(UdpSocket::bind(client_listen).await?);
        let client_addr_store: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));

        let proxy_addr = client_socket.local_addr()?;

        // Fault pipelines — one per direction, both share the same phases
        let px4_to_client = Arc::new(Mutex::new(FaultPipeline::with_phases(
            fault_profile.clone(),
            fault_phases.clone(),
        )));
        let client_to_px4 = Arc::new(Mutex::new(FaultPipeline::with_phases(
            fault_profile,
            fault_phases,
        )));

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
                    {
                        *px4_addr.lock().unwrap() = Some(src_addr);
                    }

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
                    {
                        *client_addr.lock().unwrap() = Some(src_addr);
                    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UdpSocket;
    use tokio::time::{timeout, Duration};

    /// Bind to port 0, record the OS-assigned port, drop the socket, return the port.
    /// There is a small TOCTOU window, which is acceptable in tests.
    async fn find_free_port() -> u16 {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sock.local_addr().unwrap().port()
    }

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

    /// Happy-path forwarding: packets sent from the PX4 side reach the client side.
    ///
    /// Flow:
    ///   1. Client sends a handshake to client_listen_port → proxy learns client address.
    ///   2. PX4 sim sends a numbered packet to px4_listen_port → proxy forwards to client.
    ///   3. Repeat for 5 packets; verify every payload arrives intact.
    #[tokio::test]
    async fn proxy_forwards_packets_no_faults() {
        let px4_port = find_free_port().await;
        let client_port = find_free_port().await;

        let proxy = UdpProxy::new(px4_port, client_port);
        let (_proxy_addr, _handle) = proxy.start(no_fault_profile(), vec![]).await.unwrap();

        // Give the proxy tasks time to start listening.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // "PX4 simulator" socket — will send to px4_listen_port.
        let px4_sim = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let px4_target: SocketAddr = format!("127.0.0.1:{px4_port}").parse().unwrap();

        // "Client" socket — will send/receive on client_listen_port.
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_target: SocketAddr = format!("127.0.0.1:{client_port}").parse().unwrap();

        // Step 1: client sends a handshake so the proxy learns its address.
        client.send_to(b"handshake", client_target).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Step 2-3: PX4 sends 5 distinct packets; client should receive each one.
        for i in 0u8..5 {
            let payload = [i; 8];
            px4_sim.send_to(&payload, px4_target).await.unwrap();

            let mut buf = [0u8; 256];
            let (len, _) = timeout(Duration::from_millis(500), client.recv_from(&mut buf))
                .await
                .expect("timed out waiting for forwarded packet")
                .unwrap();

            assert_eq!(len, 8);
            assert_eq!(&buf[..len], &[i; 8], "packet {i} content mismatch");
        }
    }

    /// With loss_rate = 1.0 every packet must be dropped — receives must time out.
    #[tokio::test]
    async fn proxy_drops_all_with_full_loss() {
        let px4_port = find_free_port().await;
        let client_port = find_free_port().await;

        let proxy = UdpProxy::new(px4_port, client_port);
        let full_loss = FaultProfile {
            loss_rate: 1.0,
            ..no_fault_profile()
        };
        let (_proxy_addr, _handle) = proxy.start(full_loss, vec![]).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        let px4_sim = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let px4_target: SocketAddr = format!("127.0.0.1:{px4_port}").parse().unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_target: SocketAddr = format!("127.0.0.1:{client_port}").parse().unwrap();

        // Handshake so the proxy knows the client address.
        client.send_to(b"handshake", client_target).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send 5 packets from PX4 side — all must be dropped.
        for i in 0u8..5 {
            px4_sim.send_to(&[i; 4], px4_target).await.unwrap();
        }

        // Any receive within 100 ms means a packet leaked through — that is a failure.
        let mut buf = [0u8; 256];
        let result = timeout(Duration::from_millis(100), client.recv_from(&mut buf)).await;

        assert!(
            result.is_err(),
            "expected timeout but a packet was received"
        );
    }

    /// A zero-byte UDP datagram must not crash the proxy.
    #[tokio::test]
    async fn proxy_handles_zero_byte_packet() {
        let px4_port = find_free_port().await;
        let client_port = find_free_port().await;

        let proxy = UdpProxy::new(px4_port, client_port);
        let (_proxy_addr, _handle) = proxy.start(no_fault_profile(), vec![]).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        let px4_sim = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let px4_target: SocketAddr = format!("127.0.0.1:{px4_port}").parse().unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_target: SocketAddr = format!("127.0.0.1:{client_port}").parse().unwrap();

        // Teach the proxy the client address.
        client.send_to(b"handshake", client_target).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send a zero-byte packet — the proxy must not panic or hang.
        px4_sim.send_to(&[], px4_target).await.unwrap();

        // Attempt to receive; a zero-length datagram arriving is fine, a timeout is fine too.
        let mut buf = [0u8; 256];
        let _ = timeout(Duration::from_millis(200), client.recv_from(&mut buf)).await;

        // If we reached here without a panic the test passes.
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
