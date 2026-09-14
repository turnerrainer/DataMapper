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

    /// h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N8 — a single
    /// request header exceeded the operator-configured value size
    /// cap. Hyper's default per-header cap is generous (400+ KiB);
    /// this middleware-level check rejects at the application layer
    /// with a structured 431 so an operator's WAF / log aggregator
    /// sees the standard shape.
    #[error("request header value of {actual} bytes exceeds cap {cap}")]
    HeaderValueTooLarge { actual: usize, cap: usize },

    #[error("rendered output exceeds limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },

    #[error("request body is not valid JSON: {0}")]
    InvalidJson(String),

    #[error("path traversal or invalid path segment: {0}")]
    InvalidPath(String),

    #[error("method not allowed on this route")]
    MethodNotAllowed,

    /// R2.5 / D-007: JS DataMapper's `express.urlencoded` accepted
    /// form-encoded bodies. Rust intentionally rejects them, but
    /// with a specific error naming the Content-Type so the operator
    /// can pinpoint the mismatch instead of parsing an `InvalidJson`
    /// message.
    #[error("Content-Type '{0}' is not supported — post JSON with Content-Type: application/json")]
    UnsupportedContentType(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl DataMapperError {
    fn status(&self) -> StatusCode {
        match self {
            DataMapperError::TemplateNotFound { .. } => StatusCode::NOT_FOUND,
            DataMapperError::TemplateRenderError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            DataMapperError::RequestTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            DataMapperError::HeaderValueTooLarge { .. } => {
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
            }
            DataMapperError::ResponseTooLarge { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            DataMapperError::InvalidJson(_) => StatusCode::BAD_REQUEST,
            DataMapperError::InvalidPath(_) => StatusCode::BAD_REQUEST,
            DataMapperError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            DataMapperError::UnsupportedContentType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            DataMapperError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            DataMapperError::TemplateNotFound { .. } => "TemplateNotFound",
            DataMapperError::TemplateRenderError { .. } => "TemplateRenderError",
            DataMapperError::RequestTooLarge { .. } => "RequestTooLarge",
            DataMapperError::HeaderValueTooLarge { .. } => "HeaderValueTooLarge",
            DataMapperError::ResponseTooLarge { .. } => "ResponseTooLarge",
            DataMapperError::InvalidJson(_) => "InvalidJson",
            DataMapperError::InvalidPath(_) => "InvalidPath",
            DataMapperError::MethodNotAllowed => "MethodNotAllowed",
            DataMapperError::UnsupportedContentType(_) => "UnsupportedContentType",
            DataMapperError::Internal(_) => "Internal",
        }
    }
}

/// Per-entry cap on the `tried` paths echoed in a TemplateNotFound
/// response. Each attempted lookup path is truncated at this many
/// characters before landing in the JSON body.
///
/// Rationale (h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N5): the
/// pre-fix 404 body echoed the full attempted paths verbatim, giving
/// a 4× amplification factor (10 KB URL input → ~80 KB response).
/// The URL segment is attacker-controlled — combined with the
/// absence of rate limiting inside DataMapper (out of scope; belongs
/// at the reverse proxy) this was a cheap bandwidth-amplification
/// lane. 256 chars per entry is enough for any legitimate template
/// path while capping the amplification at ~2× (URL bytes → JSON
/// bytes).
const MAX_TRIED_PATH_CHARS: usize = 256;

/// Cap each entry of a TemplateNotFound `tried` list at
/// [`MAX_TRIED_PATH_CHARS`] characters, suffixed with an ellipsis
/// marker when truncated so operators can tell at a glance that the
/// entry was clipped.
fn clip_tried_paths(tried: &[String]) -> Vec<String> {
    tried
        .iter()
        .map(|p| {
            if p.chars().count() > MAX_TRIED_PATH_CHARS {
                let mut clipped: String = p.chars().take(MAX_TRIED_PATH_CHARS).collect();
                clipped.push_str("… [truncated]");
                clipped
            } else {
                p.clone()
            }
        })
        .collect()
}

impl IntoResponse for DataMapperError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut body = json!({
            "error": self.code(),
            "message": self.to_string(),
        });

        if let DataMapperError::TemplateNotFound { tried } = &self {
            // h2ck.me v1 N5 — clip echoed paths so a 10 KB URL
            // input can't amplify into an 80 KB response body.
            body["tried"] = json!(clip_tried_paths(tried));
        }
        if let DataMapperError::RequestTooLarge { limit }
        | DataMapperError::ResponseTooLarge { limit } = &self
        {
            body["limit"] = json!(limit);
        }
        if let DataMapperError::HeaderValueTooLarge { actual, cap } = &self {
            body["actual"] = json!(actual);
            body["limit"] = json!(cap);
        }
        if let DataMapperError::TemplateRenderError { view, .. } = &self {
            body["view"] = json!(view);
        }

        (status, Json(body)).into_response()
    }
}

/// h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N6 — global fallback
/// for routes that don't match any registered handler. Pre-fix, a
/// path-encoded traversal (`%2F..%2F..%2Fetc%2Fpasswd`) hit Axum's
/// default 404 with an empty body and no content-type — surprising
/// clients that expected the same structured JSON shape as every
/// other 404 in the service. Emit the same
/// `{"error":"NotFound","message":"…"}` body so 404s are
/// indistinguishable from the router's own TemplateNotFound path.
pub async fn not_found_fallback() -> Response {
    let body = json!({
        "error": "NotFound",
        "message": "no route matches the requested method + path",
    });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n5_short_paths_unchanged() {
        let input = vec!["samples/echo.hbs".to_string()];
        assert_eq!(clip_tried_paths(&input), input);
    }

    #[test]
    fn n5_long_paths_clipped() {
        let long = "a".repeat(1000);
        let clipped = clip_tried_paths(std::slice::from_ref(&long));
        assert!(
            clipped[0].chars().count() <= MAX_TRIED_PATH_CHARS + 20,
            "clipped path too long: {}",
            clipped[0].chars().count()
        );
        assert!(
            clipped[0].contains("[truncated]"),
            "expected truncation marker: {}",
            clipped[0]
        );
    }

    #[test]
    fn n5_multibyte_char_boundary_safe() {
        // 256 × 'ä' (2-byte UTF-8) = 512 bytes but 256 chars → NOT
        // clipped. 257 × 'ä' = 514 bytes / 257 chars → clipped.
        let short = "ä".repeat(MAX_TRIED_PATH_CHARS);
        let clipped_short = clip_tried_paths(std::slice::from_ref(&short));
        assert_eq!(clipped_short[0], short, "at-cap must not clip");

        let long = "ä".repeat(MAX_TRIED_PATH_CHARS + 1);
        let clipped_long = clip_tried_paths(std::slice::from_ref(&long));
        assert!(clipped_long[0].contains("[truncated]"));
        // Char slicing means no half-byte panics.
        assert!(clipped_long[0].is_char_boundary(clipped_long[0].len()));
    }
}
