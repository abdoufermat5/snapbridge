use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("task failed: {0}")]
    TaskFailed(String),

    #[error("missing resource: {0}")]
    Missing(String),

    #[error("unexpected response: {0}")]
    Unexpected(String),

    #[error("command failed: {0}")]
    CommandFailed(String),
}
