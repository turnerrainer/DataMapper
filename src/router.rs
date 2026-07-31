//! Axum router — HTTP surface of DataMapper.
//!
//! Routes:
//! * `POST /:project/*view` — resolve `<project>/<view>.hbs` under
//!   the DSL root, render against the JSON body, negotiate the
//!   response format (see [`wants_json`]), and return.
//! * `GET  /healthz` — `{"service":"DataMapper","ok":true,"ts":…}`
//! * `GET  /health`  — same, alias for parity with other Buerostack
//!   Rust components (Ruuter, XTR).
//!
//! Output negotiation preserves the original DataMapper Node.js
//! behaviour so existing consumers work unchanged:
//! * If `type: json` header is set OR `Accept: application/json`,
//!   parse the rendered output as JSON and send as `application/json`.
//!   Fall back to raw with `application/json` MIME if parse fails.
//! * Otherwise, opportunistically parse as JSON; on success send
//!   JSON, on failure send as `text/html` (backwards compatible with
//!   the original Node.js server).

use crate::error::DataMapperError;
use crate::renderer::Renderer;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, post};
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;

#[derive(Clone)]
pub struct AppState {
    pub renderer: Arc<Renderer>,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
}

pub fn build(state: AppState) -> Router {
    let request_limit = state.max_request_bytes;
    // Coarse layer-level backstop for runaway renders. Emits 504
    // Gateway Timeout on overrun so operators can distinguish the
    // "template exploded" case (500) from the "template just took
    // too long" case (504).
    let timeout_layer =
        TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(30));
    Router::new()
        .route("/healthz", any(healthz))
        .route("/health", any(healthz))
        .route(
            "/:project/*view",
            post(invoke)
                // Give axum's default-body-limit a small headroom
                // above our authoritative check so the framework's
                // hard 413 fires only on pathological uploads; the
                // structured 413 with JSON body is produced by
                // `invoke` itself.
                .layer(DefaultBodyLimit::max(request_limit.saturating_add(4096))),
        )
        .layer(timeout_layer)
        .with_state(state)
}

async fn healthz(method: axum::http::Method) -> Response {
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return DataMapperError::MethodNotAllowed.into_response();
    }
    let body = json!({
        "service": "DataMapper",
        "ok": true,
        "ts": chrono::Utc::now().to_rfc3339(),
    });
    (StatusCode::OK, axum::Json(body)).into_response()
}

async fn invoke(
    State(state): State<AppState>,
    Path((project, view)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > state.max_request_bytes {
        return DataMapperError::RequestTooLarge {
            limit: state.max_request_bytes,
        }
        .into_response();
    }

    // Parse the body as JSON — DataMapper's contract is JSON in,
    // rendered output out. Empty body is treated as `{}` so simple
    // DSLs like `ping` work without a client having to send `{}`.
    let context: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return DataMapperError::InvalidJson(e.to_string()).into_response(),
        }
    };

    let rendered = match state.renderer.render(&project, &view, &context) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if rendered.len() > state.max_response_bytes {
        return DataMapperError::ResponseTooLarge {
            limit: state.max_response_bytes,
        }
        .into_response();
    }

    let prefers_json = wants_json(&headers);
    respond(rendered, prefers_json)
}

/// Response negotiation policy — see module doc.
fn respond(rendered: String, prefers_json: bool) -> Response {
    if prefers_json {
        return match serde_json::from_str::<Value>(&rendered) {
            Ok(v) => (StatusCode::OK, axum::Json(v)).into_response(),
            Err(_) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                rendered,
            )
                .into_response(),
        };
    }
    // Client did not signal JSON preference — opportunistically
    // detect JSON output and upgrade the MIME. Falls back to HTML
    // (compat with original DataMapper Node.js behaviour).
    match serde_json::from_str::<Value>(&rendered) {
        Ok(v) => (StatusCode::OK, axum::Json(v)).into_response(),
        Err(_) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            rendered,
        )
            .into_response(),
    }
}

/// True if the request signals a JSON preference:
/// * `type: json` custom header (original DataMapper convention), OR
/// * `Accept:` header contains `application/json` OR `*/*`.
///
/// Case-insensitive on the custom header value.
pub fn wants_json(headers: &HeaderMap) -> bool {
    if let Some(v) = headers.get("type").and_then(|v| v.to_str().ok()) {
        if v.eq_ignore_ascii_case("json") {
            return true;
        }
    }
    if let Some(v) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) {
        // Accept: */* → JSON is the safe default. Any explicit
        // `application/json` in the list wins too. HTML is only
        // preferred when the client asked exclusively for it.
        let lower = v.to_ascii_lowercase();
        if lower.contains("application/json") {
            return true;
        }
        if lower.contains("*/*") && !lower.contains("text/html") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    fn hdrs(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            let name = HeaderName::from_static(k);
            h.insert(name, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn wants_json_via_custom_header() {
        assert!(wants_json(&hdrs(&[("type", "json")])));
        assert!(wants_json(&hdrs(&[("type", "JSON")])));
        assert!(wants_json(&hdrs(&[("type", "Json")])));
    }

    #[test]
    fn wants_json_via_accept() {
        assert!(wants_json(&hdrs(&[("accept", "application/json")])));
        assert!(wants_json(&hdrs(&[(
            "accept",
            "text/plain, application/json;q=0.9"
        )])));
    }

    #[test]
    fn wants_json_via_wildcard_accept() {
        assert!(wants_json(&hdrs(&[("accept", "*/*")])));
    }

    #[test]
    fn wants_json_false_for_html_only() {
        assert!(!wants_json(&hdrs(&[("accept", "text/html")])));
    }

    #[test]
    fn wants_json_false_with_no_headers() {
        assert!(!wants_json(&hdrs(&[])));
    }

    #[test]
    fn wants_json_false_when_type_is_other() {
        assert!(!wants_json(&hdrs(&[("type", "xml")])));
    }
}
