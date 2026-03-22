use anyhow::Result; //import anyhow
use clap::Parser; //import clap for command-line argument parsing

/// PX4 Mission Harness: A tool for testing the resilience of PX4 missions.
#[derive(Parser)] // Derive the Parser trait from clap to enable command-line argument parsing
#[command(version, about)] // Automatically generate version and about information for the command-line interface
struct Cli {
    /// The scenario to execute (e.g., "run", "test", "version")
    #[arg(short, long)]
    scenario: String,
}

#[tokio::main] // Use the Tokio runtime for asynchronous execution
async fn main() -> Result<()> {
    let cli = Cli::parse(); // Parse command-line arguments into the Cli struct
    println!("PX4-Harness version: {}", px4_harness_core::version()); // Print the version of the harness from the core library
    println!("Loading scenario: {}", cli.scenario); // Print the scenario that was specified by the user
    Ok(()) // Return Ok to indicate successful execution
}
