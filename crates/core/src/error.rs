use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("camera: {0}")]
    Camera(String),
    #[error("mount: {0}")]
    Mount(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid argument: {0}")]
    Invalid(String),
    #[error("timeout waiting for frame")]
    Timeout,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
