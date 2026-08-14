use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("auth error: {0}")]
    AuthError(String),

    #[error("rate limited")]
    RateLimited,

    #[error("timeout")]
    Timeout,

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("database: {0}")]
    Database(String),

    #[error("auth: {0}")]
    Auth(String),

    #[error("validation: {0}")]
    Validation(String),

    #[error("config: {0}")]
    Config(String),
}

impl AppError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::AuthError(msg.into())
    }
}
