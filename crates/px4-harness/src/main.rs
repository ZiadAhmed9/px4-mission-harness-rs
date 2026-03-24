use anyhow::{Context, Result}; //import anyhow
use clap::Parser; //import clap for command-line argument parsing
use px4_harness_core::assertion::engine::evaluate_assertions;
use px4_harness_core::mavlink::connection::MavlinkConnection;
use px4_harness_core::scenario::ScenarioFile;
use std::path::PathBuf; //import PathBuf for handling file paths

use px4_harness_core::mission::controller::MissionController;
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

    println!("Connecting to PX4 SITL...");
    // Single connection, shared via Arc for both sending and receiving
    let conn = Arc::new(MavlinkConnection::connect("udpin:0.0.0.0:14540")?);

    // Start receiving messages in background (uses Arc::clone internally)
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
