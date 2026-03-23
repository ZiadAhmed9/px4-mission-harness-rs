use anyhow::{Context, Result}; //import anyhow
use clap::Parser; //import clap for command-line argument parsing
use mavlink::ardupilotmega::MavMessage; // MAVLink message enum (HEARTBEAT, etc.)
use px4_harness_core::mavlink::connection::MavlinkConnection; // our MAVLink connection wrapper
use px4_harness_core::scenario::ScenarioFile;
use std::path::PathBuf; //import PathBuf for handling file paths

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
    let conn = MavlinkConnection::connect("udpin:0.0.0.0:14540")?;
    println!("Connected. Waiting for heartbeats...");

    loop {
        match conn.recv() {
            Ok((header, MavMessage::HEARTBEAT(heartbeat))) => {
                println!(
                    "HEARTBEAT from system {} component {}: type={}, autopilot={}, mode={}",
                    header.system_id,
                    header.component_id,
                    heartbeat.mavtype as u8,
                    heartbeat.autopilot as u8,
                    heartbeat.custom_mode,
                );
            }
            Ok((header, msg)) => {
                // Print a summary of non-heartbeat messages
                println!(
                    "Message from system {} component {}: {:?}",
                    header.system_id,
                    header.component_id,
                    std::mem::discriminant(&msg),
                );
            }
            Err(e) => {
                eprintln!("Error receiving message: {}", e);
            }
        }
    }
}
