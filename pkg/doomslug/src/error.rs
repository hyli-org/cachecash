#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid signature")]
    InvalidSignature,
}
