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
//!   JSON, on failure send raw. The fallback `Content-Type` is
//!   `text/html` **only** when the client explicitly signalled
//!   `Accept: text/html`; otherwise `text/plain; charset=utf-8` is
//!   used so a mis-authored template can't hand attacker-influenced
//!   bytes to a browser as executable markup (h2ck.me v1 M2).

use crate::error::DataMapperError;
use crate::renderer::Renderer;
use axum::body::to_bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
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
    /// Wall-clock cap on how long the client has to finish sending
    /// the request body. Same value as `limits.request_timeout_secs`
    /// by default. Distinct concept from the render / response
    /// timeout (both currently share the value) — the extractor
    /// applies this cap around `to_bytes` so a slow-drip body cannot
    /// keep a Tokio task slot occupied indefinitely (h2ck.me v1 N3).
    pub request_timeout_secs: u64,
}

pub fn build(state: AppState) -> Router {
    let request_limit = state.max_request_bytes;
    // Coarse layer-level backstop for runaway renders. Emits 504
    // Gateway Timeout on overrun so operators can distinguish the
    // "template exploded" case (500) from the "template just took
    // too long" case (504). Reads from `limits.request_timeout_secs`
    // (was hard-coded 30s before N3 fix — the config field is now
    // actually wired end-to-end).
    let timeout_layer = TimeoutLayer::with_status_code(
        StatusCode::GATEWAY_TIMEOUT,
        Duration::from_secs(state.request_timeout_secs),
    );
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
    request: Request,
) -> Response {
    // h2ck.me v1 N3 — the pre-fix code used `body: Bytes` and
    // relied on Axum's built-in extractor to buffer the body. The
    // TimeoutLayer at the outer edge only starts its clock once the
    // service future runs, but the body-read future runs INSIDE that
    // service future — meaning a slow-drip body could keep the
    // socket + Tokio task open for the whole read phase without ever
    // being cancelled by the layer. Extract explicitly with our own
    // `tokio::time::timeout` so the deadline covers exactly the
    // period during which the socket is held open by an incomplete
    // request.
    //
    // The size cap in `to_bytes(_, limit)` is authoritative for
    // over-size 413 — the `DefaultBodyLimit` layer above sits at
    // `limit + 4096` so its hard 413 only fires on pathological
    // uploads (the structured JSON 413 is emitted here).
    let deadline = Duration::from_secs(state.request_timeout_secs);
    let body_fut = to_bytes(request.into_body(), state.max_request_bytes);
    let body = match tokio::time::timeout(deadline, body_fut).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            // `to_bytes` returns Err on either size overflow or a
            // client-side transport error. Both surface as `Error`;
            // treat any Err as a too-large or malformed body. The
            // structured 413 with `limit` matches the pre-fix wire
            // shape for the size-overflow case.
            let msg = e.to_string();
            if msg.contains("length limit") || msg.contains("too large") {
                return DataMapperError::RequestTooLarge {
                    limit: state.max_request_bytes,
                }
                .into_response();
            }
            return DataMapperError::Internal(format!("reading request body: {msg}"))
                .into_response();
        }
        Err(_) => {
            tracing::warn!(
                deadline_secs = state.request_timeout_secs,
                "request body read exceeded deadline — closing connection (h2ck.me v1 N3)"
            );
            return DataMapperError::RequestReadTimeout {
                deadline_secs: state.request_timeout_secs,
            }
            .into_response();
        }
    };
    if body.len() > state.max_request_bytes {
        return DataMapperError::RequestTooLarge {
            limit: state.max_request_bytes,
        }
        .into_response();
    }

    // R2.5 / D-007: JS DataMapper's `express.urlencoded` accepted
    // form-encoded bodies. Rust intentionally rejects them, but
    // with a specific 415 UnsupportedContentType naming the type
    // so the operator pinpoints the mismatch instead of parsing a
    // misleading `InvalidJson` message.
    //
    // Policy:
    //   - Empty body → skip the check (JS parity: `ping` with no body).
    //   - No Content-Type header → skip (client probably sent JSON).
    //   - `application/json`, `text/json`, `application/…+json` → allow.
    //   - Everything else → 415 naming the type.
    if !body.is_empty() {
        if let Some(ct) = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            let lower = ct.to_ascii_lowercase();
            let media = lower.split(';').next().unwrap_or("").trim();
            let json_shaped = media == "application/json"
                || media == "text/json"
                || media.ends_with("+json")
                || media.is_empty();
            if !json_shaped {
                return DataMapperError::UnsupportedContentType(media.to_string()).into_response();
            }
        }
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

    // Cap enforcement lives inside the renderer (h2ck.me v1 M3):
    // `Renderer::render` streams into a `CappedWriter` and returns
    // `ResponseTooLarge` the moment the buffer would cross the
    // limit, so an amplification template can't briefly allocate
    // multiples of the cap before we notice.
    let rendered = match state
        .renderer
        .render(&project, &view, &context, state.max_response_bytes)
    {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let prefers_json = wants_json(&headers);
    let accepts_html = accepts_html(&headers);
    respond(rendered, prefers_json, accepts_html)
}

/// Response negotiation policy — see module doc.
fn respond(rendered: String, prefers_json: bool, accepts_html: bool) -> Response {
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
    // detect JSON output and upgrade the MIME. On non-JSON output
    // the fallback is `text/html` **only** when the caller
    // explicitly asked for it via `Accept: text/html`; otherwise
    // `text/plain` so a template author's mistake can't feed
    // attacker-influenced markup to a browser (h2ck.me v1 M2).
    match serde_json::from_str::<Value>(&rendered) {
        Ok(v) => (StatusCode::OK, axum::Json(v)).into_response(),
        Err(_) => {
            let ct = if accepts_html {
                "text/html; charset=utf-8"
            } else {
                "text/plain; charset=utf-8"
            };
            (StatusCode::OK, [(header::CONTENT_TYPE, ct)], rendered).into_response()
        }
    }
}

/// True iff the client explicitly listed `text/html` in `Accept`.
/// A missing header, `*/*`, or an unrelated MIME does NOT count —
/// see the M2 rationale in the module doc.
pub fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
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

    #[test]
    fn accepts_html_only_when_explicitly_requested() {
        assert!(accepts_html(&hdrs(&[("accept", "text/html")])));
        assert!(accepts_html(&hdrs(&[(
            "accept",
            "text/html,application/xhtml+xml"
        )])));
        // Explicit browser-style Accept still counts.
        assert!(accepts_html(&hdrs(&[(
            "accept",
            "text/html;q=0.9,application/json;q=0.1"
        )])));
    }

    #[test]
    fn accepts_html_false_for_wildcard_and_missing() {
        // `*/*` alone must NOT be treated as an HTML opt-in — that
        // was the M2 bug: browsers/proxies frequently send `*/*` and
        // we'd otherwise fall through to `text/html`.
        assert!(!accepts_html(&hdrs(&[("accept", "*/*")])));
        assert!(!accepts_html(&hdrs(&[])));
        assert!(!accepts_html(&hdrs(&[("accept", "application/json")])));
    }
}
