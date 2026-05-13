use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("stripe error: {0}")]
    Stripe(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AccountError {
    fn into_response(self) -> Response {
        let status = match &self {
            AccountError::NotFound => StatusCode::NOT_FOUND,
            AccountError::Forbidden => StatusCode::FORBIDDEN,
            AccountError::Stripe(_) => StatusCode::BAD_GATEWAY,
            AccountError::Storage(_) | AccountError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

impl From<anyhow::Error> for AccountError {
    fn from(e: anyhow::Error) -> Self {
        AccountError::Internal(e.to_string())
    }
}
