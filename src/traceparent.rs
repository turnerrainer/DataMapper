//! W3C Trace Context response headers.
//!
//! Adopts FLEET-STRONGHOLDS §1.6 — the cross-service pattern for
//! cross-service correlation. When Service A calls Service B, if
//! both services emit trace ids independently there's no way to
//! correlate the two log streams. W3C Trace Context (`traceparent`
//! HTTP header + `x-trace-id` for grep-ability) standardises this.
//!
//! Behaviour:
//! - Extract inbound `traceparent` header. If well-formed (RFC 9110-
//!   compatible: `version-traceid-spanid-flags`, all hex, non-zero
//!   trace id), inherit its `trace_id`.
//! - Otherwise, generate a fresh 32-hex trace id (128 bits of
//!   `uuid::Uuid::new_v4`).
//! - Always generate a fresh 16-hex span id per response (this is
//!   THIS service's span, not the caller's).
//! - Emit both `traceparent: 00-<trace_id>-<span_id>-01` and
//!   `x-trace-id: <trace_id>` on every response. `x-trace-id` is
//!   redundant with `traceparent` but is what most operator log-grep
//!   patterns look for.
//!
//! The trace id inherited here matches the one the access-log
//! middleware writes so log aggregators can correlate one request
//! across services.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

/// The `version` byte for the W3C Trace Context spec that ships
/// today (`00`).
const VERSION: &str = "00";

/// The `trace-flags` byte — `01` means "sampled". DataMapper is
/// unauth and sits behind a reverse proxy that owns sampling, so we
/// always mark as sampled and let the collector decide.
const FLAGS: &str = "01";

/// Wire via
/// `Router::layer(axum::middleware::from_fn(traceparent))`.
pub async fn traceparent(req: Request, next: Next) -> Response {
    // Extract or generate.
    let inbound = req
        .headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_trace_id);
    let trace_id = inbound.unwrap_or_else(fresh_trace_id);
    let span_id = fresh_span_id();
    let traceparent_val = format!("{VERSION}-{trace_id}-{span_id}-{FLAGS}");

    let mut response = next.run(req).await;
    let h = response.headers_mut();
    // `insert` not `entry().or_insert()` — we own the trace ID
    // shape end-to-end. A downstream handler emitting its own
    // traceparent would confuse the aggregator; unify on ours.
    if let Ok(v) = HeaderValue::from_str(&traceparent_val) {
        h.insert("traceparent", v);
    }
    if let Ok(v) = HeaderValue::from_str(&trace_id) {
        h.insert("x-trace-id", v);
    }
    response
}

/// Extract the 32-hex trace id from an inbound `traceparent`
/// header. Returns `None` when the header is malformed, the version
/// is unsupported, or the trace id is all-zeros (reserved).
///
/// Spec: <https://www.w3.org/TR/trace-context/#traceparent-header>
pub fn parse_trace_id(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw.trim().split('-').collect();
    if parts.len() != 4 {
        return None;
    }
    let (version, trace_id, span_id, _flags) = (parts[0], parts[1], parts[2], parts[3]);
    if version != VERSION {
        return None;
    }
    if trace_id.len() != 32
        || span_id.len() != 16
        || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
        || !span_id.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    // The spec reserves an all-zero trace-id as invalid.
    if trace_id.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(trace_id.to_ascii_lowercase())
}

fn fresh_trace_id() -> String {
    // uuid v4 has 128 bits of randomness — the exact size of a W3C
    // trace id.
    uuid::Uuid::new_v4().simple().to_string()
}

fn fresh_span_id() -> String {
    // W3C spans are 64 bits = 16 hex chars. Take the first 16 hex
    // chars of a fresh uuid.
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    uuid[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_traceparent_returns_trace_id() {
        let tp = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        assert_eq!(
            parse_trace_id(tp).as_deref(),
            Some("0af7651916cd43dd8448eb211c80319c")
        );
    }

    #[test]
    fn parse_accepts_uppercase_hex_and_lowercases() {
        let tp = "00-0AF7651916CD43DD8448EB211C80319C-B7AD6B7169203331-01";
        assert_eq!(
            parse_trace_id(tp).as_deref(),
            Some("0af7651916cd43dd8448eb211c80319c")
        );
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        // Version "ff" is invalid per the spec.
        assert_eq!(
            parse_trace_id("ff-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
            None
        );
    }

    #[test]
    fn parse_rejects_wrong_field_count() {
        assert_eq!(parse_trace_id(""), None);
        assert_eq!(parse_trace_id("00"), None);
        assert_eq!(parse_trace_id("00-abc-def"), None);
        assert_eq!(parse_trace_id("00-abc-def-01-extra"), None);
    }

    #[test]
    fn parse_rejects_all_zero_trace_id() {
        // Reserved by the spec as "no valid trace id".
        assert_eq!(
            parse_trace_id("00-00000000000000000000000000000000-b7ad6b7169203331-01"),
            None
        );
    }

    #[test]
    fn parse_rejects_wrong_length_ids() {
        // trace id 31 chars instead of 32.
        assert_eq!(
            parse_trace_id("00-0af7651916cd43dd8448eb211c80319-b7ad6b7169203331-01"),
            None
        );
        // span id 15 chars instead of 16.
        assert_eq!(
            parse_trace_id("00-0af7651916cd43dd8448eb211c80319c-b7ad6b716920333-01"),
            None
        );
    }

    #[test]
    fn parse_rejects_non_hex_chars() {
        assert_eq!(
            parse_trace_id("00-0af7651916cd43dd8448eb211c8031Zz-b7ad6b7169203331-01"),
            None
        );
    }

    #[test]
    fn fresh_trace_id_is_32_hex() {
        let id = fresh_trace_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fresh_span_id_is_16_hex() {
        let id = fresh_span_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
