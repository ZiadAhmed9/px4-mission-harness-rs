//! px4-harness-core: Core logic for PX4 mission resilience testing.

// A function that returns a string literal containing the version of the crate, obtained from the environment variable set by Cargo at compile time.
pub mod assertion;
pub mod error;
pub mod fault;
pub mod mavlink;
pub mod mission;
pub mod proxy;
pub mod report;
pub mod scenario;
pub mod suite;
pub mod telemetry;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
