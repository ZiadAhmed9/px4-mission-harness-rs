//! px4-harness-core: Core logic for PX4 mission resilience testing.

// A function that returns a string literal containing the version of the crate, obtained from the environment variable set by Cargo at compile time.
pub mod error;
pub mod mavlink;
pub mod scenario;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
