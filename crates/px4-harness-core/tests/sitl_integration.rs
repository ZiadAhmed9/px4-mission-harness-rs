//! Integration tests that require a running PX4 SITL instance.
//! Run with: cargo test -p px4-harness-core --features sitl -- --nocapture

#![cfg(feature = "sitl")]

use std::path::Path;
use std::sync::Arc;

use px4_harness_core::assertion::engine::evaluate_assertions;
use px4_harness_core::mavlink::connection::MavlinkConnection;
use px4_harness_core::mission::controller::MissionController;
use px4_harness_core::proxy::udp_proxy::UdpProxy;
use px4_harness_core::scenario::ScenarioFile;

#[tokio::test]
async fn simple_mission_completes() {
    let scenario = ScenarioFile::load(Path::new("../../scenarios/simple_mission.toml"))
        .expect("failed to load scenario");

    let proxy = UdpProxy::new(14550, 14570);
    let (_addr, _handle) = proxy
        .start(scenario.faults.clone(), scenario.fault_phases.clone())
        .await
        .expect("failed to start proxy");

    let conn =
        Arc::new(MavlinkConnection::connect("udpout:127.0.0.1:14570").expect("failed to connect"));
    let rx = conn.start_recv_task();
    let controller = MissionController::new(Arc::clone(&conn));

    let store = controller
        .run_mission(&scenario.mission, rx, false)
        .await
        .expect("mission failed");

    let results = evaluate_assertions(&scenario.assertions, &scenario.mission.waypoints, &store);

    for result in &results {
        assert!(
            result.passed,
            "Assertion failed: {}: {}",
            result.name, result.reason
        );
    }
}
