use anyhow::{Context, Result}; //import anyhow
use clap::Parser; //import clap for command-line argument parsing
use px4_harness_core::assertion::engine::evaluate_assertions;
use px4_harness_core::mavlink::connection::MavlinkConnection;
use px4_harness_core::mission::controller::MissionController;
use px4_harness_core::proxy::udp_proxy::UdpProxy;
use px4_harness_core::scenario::ScenarioFile;
use std::path::PathBuf;
use std::sync::Arc;

/// PX4 Mission Harness: A tool for testing the resilience of PX4 missions.
#[derive(Parser)] // Derive the Parser trait from clap to enable command-line argument parsing
#[command(version, about)] // Automatically generate version and about information for the command-line interface
struct Cli {
    /// The scenario to execute (e.g., "run", "test", "version")
    #[arg(short, long)]
    scenario: PathBuf, // Define a command-line argument named "scenario" that accepts a file path
}

#[tokio::main] // Use the Tokio runtime for asynchronous execution
async fn main() -> Result<()> {
    let cli = Cli::parse(); // Parse command-line arguments into the Cli struct
    println!("PX4-Harness version: {}", px4_harness_core::version()); // Print the version of the harness from the core library

    // Load the scenario file specified by the user and handle any errors that may occur during loading
    let scenario = ScenarioFile::load(&cli.scenario).context("failed to load scenario file")?;

    println!("Loaded scenario: {}", scenario.scenario.name);
    println!("  Waypoints: {}", scenario.mission.waypoints.len());
    println!("  Loss rate: {}%", scenario.faults.loss_rate * 100.0);
    println!("  Assertions: {}", scenario.assertions.len());

    // Start the fault injection proxy between PX4 and our harness.
    // PX4 SITL sends GCS MAVLink to port 14550, so the proxy listens there.
    // Our harness connects to the proxy on port 14560.
    let proxy = UdpProxy::new(14550, 14560);
    let (proxy_addr, _proxy_handle) = proxy
        .start(scenario.faults.clone())
        .await
        .context("failed to start proxy")?;
    println!("Proxy: PX4→14550→[faults]→{}→harness", proxy_addr);

    // Connect to the proxy's client port
    println!("Connecting to PX4 SITL (through proxy)...");
    let conn = Arc::new(MavlinkConnection::connect("udpout:127.0.0.1:14560")?);

    // Start receiving messages in background
    let rx = conn.start_recv_task();

    // Create mission controller and run
    let controller = MissionController::new(Arc::clone(&conn));

    let store = controller.run_mission(&scenario.mission, rx).await?;

    // Print telemetry summary
    {
        let positions = store.positions.lock().unwrap();
        let attitudes = store.attitudes.lock().unwrap();
        let statuses = store.statuses.lock().unwrap();
        println!("\nTelemetry summary:");
        println!("  Position samples: {}", positions.len());
        println!("  Attitude samples: {}", attitudes.len());
        println!("  Status samples: {}", statuses.len());
        if let Some(last) = positions.last() {
            println!(
                "  Final position: ({:.6}, {:.6}) alt={:.1}m",
                last.latitude, last.longitude, last.relative_alt
            );
        }
    } // locks released here

    // Evaluate assertions against collected telemetry
    let results = evaluate_assertions(
        &scenario.assertions,
        &scenario.mission.waypoints,
        &store,
    );

    println!("\n=== Assertion Results ===");
    let mut passed = 0;
    let mut failed = 0;
    for result in &results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        println!("  [{}] {}: {}", status, result.name, result.reason);
        if result.passed {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    println!("\n{} passed, {} failed, {} total", passed, failed, results.len());

    Ok(())
}
