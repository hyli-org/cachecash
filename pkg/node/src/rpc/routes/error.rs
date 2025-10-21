use std::num::ParseIntError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Element expected to be a valid U256 integer, got {0}")]
    InvalidElement(String, #[source] ParseIntError),

    #[error("Out of sync")]
    OutOfSync,

    #[error("Statistics are not ready")]
    StatisticsNotReady,

    #[error("Invalid list query")]
    InvalidListQuery(#[source] serde_json::Error),
}
