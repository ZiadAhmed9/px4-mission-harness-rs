//! px4-harness-core: Core logic for PX4 mission resilience testing.

pub mod assertion;
pub mod error;
pub mod event;
pub mod fault;
pub mod generate;
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
