// library crates define typed errors so callers can handle each case.
use thiserror::Error; // Import the thiserror crate for easy error handling

#[derive(Debug, Error)] // Derive the Error and Debug traits for the HarnessError enum
pub enum HarnessError {
    #[error("failed to read scenario file: {path}")]
    // Define an error variant for file reading issues, with a message that includes the file path
    ScenarioFileRead {
        path: String, // The path of the scenario file that failed to read
        #[source]
        source: std::io::Error, // Include the source error for better debugging
    },

    #[error("failed to parse scenario TOML")]
    ScenarioParse(#[from] toml::de::Error),

    #[error("scenario validation failed: {reason}")]
    // Define an error variant for scenario validation failures, with a message that includes the reason for failure
    ScenarioValidation { reason: String }, // The reason why the scenario validation failed

    #[error("failed to connect to MAVLink at {address}")]
    MavlinkConnection {
        address: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to receive MAVLink message")]
    MavlinkReceive {
        #[source]
        source: mavlink::error::MessageReadError,
    },

    #[error("failed to send MAVLink message")]
    MavlinkSend {
        #[source]
        source: mavlink::error::MessageWriteError,
    },
}
