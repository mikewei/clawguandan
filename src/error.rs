use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {message}")]
    Conflict {
        message: String,
        code: &'static str,
        current_seq: Option<u64>,
    },
    #[error("unprocessable: {message}")]
    Unprocessable {
        message: String,
        code: &'static str,
        current_seq: Option<u64>,
    },
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: ErrorPayload,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_seq: Option<u64>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub details: serde_json::Map<String, serde_json::Value>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, current_seq) = match &self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", m.clone(), None),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, "NOT_FOUND", m.clone(), None),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, "FORBIDDEN", m.clone(), None),
            AppError::Conflict {
                message,
                code,
                current_seq,
            } => (
                StatusCode::CONFLICT,
                *code,
                message.clone(),
                *current_seq,
            ),
            AppError::Unprocessable {
                message,
                code,
                current_seq,
            } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                *code,
                message.clone(),
                *current_seq,
            ),
        };

        let body = ErrorBody {
            error: ErrorPayload {
                code: code.to_string(),
                message,
                current_seq,
                details: serde_json::Map::new(),
            },
        };

        (status, Json(body)).into_response()
    }
}
