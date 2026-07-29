//! Domain error type.
//!
//! `DataMapperError` is the library-facing error enum. Each variant
//! carries the operator-actionable context (which template, which
//! limit, which path) and maps to an HTTP status + JSON body via
//! `IntoResponse`. Keep the enum small — variants are the API.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum DataMapperError {
    #[error("template not found: tried {tried:?}")]
    TemplateNotFound { tried: Vec<String> },

    #[error("template render error in {view}: {message}")]
    TemplateRenderError { view: String, message: String },

    #[error("request body exceeds limit of {limit} bytes")]
    RequestTooLarge { limit: usize },

    #[error("rendered output exceeds limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },

    #[error("request body is not valid JSON: {0}")]
    InvalidJson(String),

    #[error("path traversal or invalid path segment: {0}")]
    InvalidPath(String),

    #[error("method not allowed on this route")]
    MethodNotAllowed,

    #[error("internal error: {0}")]
    Internal(String),
}

impl DataMapperError {
    fn status(&self) -> StatusCode {
        match self {
            DataMapperError::TemplateNotFound { .. } => StatusCode::NOT_FOUND,
            DataMapperError::TemplateRenderError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            DataMapperError::RequestTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            DataMapperError::ResponseTooLarge { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            DataMapperError::InvalidJson(_) => StatusCode::BAD_REQUEST,
            DataMapperError::InvalidPath(_) => StatusCode::BAD_REQUEST,
            DataMapperError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            DataMapperError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            DataMapperError::TemplateNotFound { .. } => "TemplateNotFound",
            DataMapperError::TemplateRenderError { .. } => "TemplateRenderError",
            DataMapperError::RequestTooLarge { .. } => "RequestTooLarge",
            DataMapperError::ResponseTooLarge { .. } => "ResponseTooLarge",
            DataMapperError::InvalidJson(_) => "InvalidJson",
            DataMapperError::InvalidPath(_) => "InvalidPath",
            DataMapperError::MethodNotAllowed => "MethodNotAllowed",
            DataMapperError::Internal(_) => "Internal",
        }
    }
}

impl IntoResponse for DataMapperError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });

        if let DataMapperError::TemplateNotFound { tried } = &self {
            body["tried"] = json!(tried);
        }
        if let DataMapperError::RequestTooLarge { limit }
        | DataMapperError::ResponseTooLarge { limit } = &self
        {
            body["limit"] = json!(limit);
        }
        if let DataMapperError::TemplateRenderError { view, .. } = &self {
            body["view"] = json!(view);
        }

        (status, Json(body)).into_response()
    }
}
