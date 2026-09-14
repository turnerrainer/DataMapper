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
        // Audit LOG-v1 FN-LOG-2: emit one INFO line per completed request
        // for SOC2/ISO27001 access-log compliance. See src/access_log.rs.
        .layer(axum::middleware::from_fn(
            crate::access_log::access_log_middleware,
        ))
        // FLEET-STRONGHOLDS §5.1 — five default security headers
        // (CSP, HSTS, X-Frame-Options, X-Content-Type-Options,
        // Referrer-Policy) on every response including error paths.
        // Applied AFTER handler routing so error handlers and
        // fallbacks also inherit them.
        .layer(axum::middleware::from_fn(
            crate::security_headers::security_headers,
        ))
        // FLEET-STRONGHOLDS §1.6 — W3C Trace Context response
        // headers. Every response carries `traceparent:
        // 00-<trace>-<span>-01` and `x-trace-id: <trace>`. Inbound
        // `traceparent` is inherited when well-formed; a fresh
        // 32-hex uuid is generated otherwise. Enables end-to-end
        // request correlation across the fleet.
        .layer(axum::middleware::from_fn(crate::traceparent::traceparent))
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

/// True iff the client's `Accept` header lists `text/html`
/// **explicitly** with a non-zero quality value. `*/*` alone does NOT
/// count — the M2 rule is "HTML fallback requires an explicit opt-in
/// because browsers routinely send `*/*` and would otherwise get
/// executable markup back from a mis-authored template."
///
/// Bare `contains("text/html")` was the original M2 check, but
/// h2ck.me v1 break-test N2 showed two bypasses: `Accept:
/// text/html;q=0` (explicitly excluded per RFC 7231 §5.3.1) still
/// matched, and `Accept: application/json;q=0, text/html`
/// legitimately meaning "give me JSON, definitely not HTML" also
/// matched and fell through to HTML.
///
/// Semantics here: parse the Accept header per RFC 7231 §5.3.2 and
/// return true iff `text/html` (exact) or `text/*` (type-wildcard)
/// appears with `q > 0`.
pub fn accepts_html(headers: &HeaderMap) -> bool {
    accept_qvalue(headers, "text/html", MatchSpecificity::TypeWildcard)
        .map(|q| q > 0.0)
        .unwrap_or(false)
}

/// True if the request signals a JSON preference:
/// * `type: json` custom header (original DataMapper convention), OR
/// * `Accept:` header lists `application/json` with `q > 0` at least
///   as preferred as `text/html`, OR
/// * `Accept: */*` (safe default) AND `text/html` is not present
///   with a strictly higher `q` than JSON.
///
/// Case-insensitive on the custom header value. The q-value logic
/// closes the h2ck.me v1 N2 bypass where a bare-string check for
/// `application/json` would treat `application/json;q=0, text/html`
/// as "JSON preferred" — the caller in fact excluded JSON.
pub fn wants_json(headers: &HeaderMap) -> bool {
    if let Some(v) = headers.get("type").and_then(|v| v.to_str().ok()) {
        if v.eq_ignore_ascii_case("json") {
            return true;
        }
    }
    let json_q =
        accept_qvalue(headers, "application/json", MatchSpecificity::TypeWildcard).unwrap_or(0.0);
    let html_q = accept_qvalue(headers, "text/html", MatchSpecificity::TypeWildcard).unwrap_or(0.0);
    // Universal wildcard is only consulted for the "safe JSON default"
    // branch — it must not turn on the HTML fallback.
    let star_q =
        accept_qvalue(headers, "application/json", MatchSpecificity::Universal).unwrap_or(0.0);
    if json_q > 0.0 && json_q >= html_q {
        return true;
    }
    // `*/*` with no explicit media type: JSON is the safe default,
    // but only when HTML wasn't listed with a strictly higher q.
    if star_q > 0.0 && json_q == 0.0 && html_q == 0.0 {
        return true;
    }
    false
}

/// How specific a match must be to count when resolving an
/// `Accept` header's q-value. `TypeWildcard` allows exact
/// (`text/html`) and type-wildcard (`text/*`) matches. `Universal`
/// additionally allows `*/*` as a fallback match — used only where
/// the caller wants "JSON is the safe default under `*/*`".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchSpecificity {
    TypeWildcard,
    Universal,
}

/// Look up the effective quality value the client assigned to
/// `wanted_media` in the `Accept` header, per RFC 7231 §5.3.1 /
/// §5.3.2. Returns `None` when the header is absent, unparseable, or
/// the media is not listed at the required minimum specificity.
///
/// Matching rules (most-specific wins):
/// - Exact media (`text/html` matches `text/html`) always wins.
/// - Type wildcard (`text/*` matches `text/html`) matches when
///   nothing more specific applies.
/// - Universal wildcard (`*/*`) matches only when
///   `min_specificity == Universal`.
/// - A missing `q=` parameter defaults to `1.0`; a malformed value
///   defaults to `1.0` (RFC advice — ignore invalid params, don't
///   reject the entry). Values outside `[0, 1]` are clamped.
/// - Case-insensitive on the media token, per RFC 9110 §5.1.
fn accept_qvalue(
    headers: &HeaderMap,
    wanted_media: &str,
    min_specificity: MatchSpecificity,
) -> Option<f32> {
    let raw = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok())?;
    let wanted_lower = wanted_media.to_ascii_lowercase();
    let (wanted_type, wanted_subtype) = wanted_lower.split_once('/')?;
    let mut best: Option<(u8, f32)> = None; // (specificity, q)
    for raw_entry in raw.split(',') {
        let mut parts = raw_entry.split(';').map(str::trim);
        let media = parts.next().unwrap_or("").to_ascii_lowercase();
        let (etype, esub) = match media.split_once('/') {
            Some(pair) => pair,
            None => continue,
        };
        let specificity: u8 = match (etype, esub) {
            (t, s) if t == wanted_type && s == wanted_subtype => 3,
            (t, "*") if t == wanted_type => 2,
            ("*", "*") if min_specificity == MatchSpecificity::Universal => 1,
            _ => continue,
        };
        // Parse ;q=<value> if present; default 1.0. RFC 7231
        // constrains q to [0, 1] with at most three decimal digits,
        // but we tolerate any parseable f32 and clamp defensively.
        let mut q: f32 = 1.0;
        for param in parts {
            if let Some(val) = param
                .strip_prefix("q=")
                .or_else(|| param.strip_prefix("Q="))
            {
                q = val.trim().parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0);
            }
        }
        best = Some(match best {
            None => (specificity, q),
            Some((prev_spec, prev_q))
                if specificity > prev_spec || (specificity == prev_spec && q > prev_q) =>
            {
                (specificity, q)
            }
            Some(prev) => prev,
        });
    }
    best.map(|(_, q)| q)
}

/// Look up the effective quality value the client assigned to
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

    // ---------- N2 (Accept q-values ignored → M2 bypass) ----------
    //
    // h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N2 upgraded the
    // audit's L3 (Accept q-values ignored) from LOW to MEDIUM because
    // the M2 fix's bare-string `contains("text/html")` treated
    // `Accept: text/html;q=0` as a legitimate HTML opt-in — turning
    // an RFC-compliant "definitely not HTML" client into an XSS
    // amplifier for a bad triple-brace template.

    #[test]
    fn n2_html_q_zero_is_not_accepted_as_html() {
        // RFC 7231 §5.3.1: q=0 means "not acceptable".
        assert!(!accepts_html(&hdrs(&[("accept", "text/html;q=0")])));
        assert!(!accepts_html(&hdrs(&[(
            "accept",
            "application/json, text/html;q=0"
        )])));
    }

    #[test]
    fn n2_json_q_zero_disqualifies_wants_json() {
        // Caller: "give me HTML, definitely not JSON."
        // Was: bare-string `contains("application/json")` → wants_json=true (bug)
        // Now: json q=0 → wants_json=false; html q=1 → accepts_html=true.
        assert!(!wants_json(&hdrs(&[(
            "accept",
            "application/json;q=0, text/html"
        )])));
        assert!(accepts_html(&hdrs(&[(
            "accept",
            "application/json;q=0, text/html"
        )])));
    }

    #[test]
    fn n2_qvalues_pick_higher_preference() {
        // Caller: "html at 1.0, json at 0.5" — html wins by q.
        // wants_json must return false since json is available but
        // lower-preference than html.
        assert!(!wants_json(&hdrs(&[(
            "accept",
            "text/html;q=1.0, application/json;q=0.5"
        )])));
        assert!(accepts_html(&hdrs(&[(
            "accept",
            "text/html;q=1.0, application/json;q=0.5"
        )])));
    }

    #[test]
    fn n2_json_higher_preference_selects_json() {
        // Inverse of the above — json at 1.0, html at 0.5.
        assert!(wants_json(&hdrs(&[(
            "accept",
            "text/html;q=0.5, application/json;q=1.0"
        )])));
    }

    #[test]
    fn n2_html_q_partial_is_still_accepted() {
        // `text/html;q=0.9` — legitimate partial preference, still
        // > 0, still counts as HTML opt-in.
        assert!(accepts_html(&hdrs(&[("accept", "text/html;q=0.9")])));
    }

    #[test]
    fn n2_qvalue_case_insensitive_and_whitespace_tolerant() {
        // RFC 7231: parameter names are case-insensitive; OWS
        // permitted around `;` and `=`.
        assert!(accepts_html(&hdrs(&[("accept", "text/html; Q=0.5")])));
        assert!(!accepts_html(&hdrs(&[("accept", "text/html ; q=0")])));
    }

    #[test]
    fn n2_wildcard_star_star_matches_when_specific_absent() {
        // `*/*` alone → both accepts_html and json's is-preferred
        // path resolve via the wildcard. Preserves M2 posture: json
        // is the safe default under `*/*` while HTML fallback stays
        // off.
        assert!(wants_json(&hdrs(&[("accept", "*/*")])));
        assert!(!accepts_html(&hdrs(&[("accept", "*/*")])));
    }

    #[test]
    fn n2_type_wildcard_text_star_matches_text_html() {
        // `text/*` covers `text/html` at the type level.
        assert!(accepts_html(&hdrs(&[("accept", "text/*")])));
    }

    #[test]
    fn n2_malformed_qvalue_defaults_to_one() {
        // RFC advice: ignore malformed parameter values; don't reject
        // the whole entry. So `text/html;q=banana` still counts.
        assert!(accepts_html(&hdrs(&[("accept", "text/html;q=banana")])));
    }

    #[test]
    fn n2_qvalue_out_of_range_is_clamped() {
        // Some quality libraries emit `q=1.5` or `q=-0.1`; RFC says
        // out-of-range is invalid, but we clamp defensively so a
        // pathological value can't invert the check.
        assert!(accepts_html(&hdrs(&[("accept", "text/html;q=1.5")])));
        assert!(!accepts_html(&hdrs(&[("accept", "text/html;q=-0.1")])));
    }

    #[test]
    fn n2_most_specific_entry_wins_over_wildcard() {
        // `text/html;q=0, */*;q=1` — the most-specific `text/html`
        // wins with q=0 → NOT accepted, even though the wildcard
        // would otherwise match.
        assert!(!accepts_html(&hdrs(&[(
            "accept",
            "text/html;q=0, */*;q=1"
        )])));
    }
}
