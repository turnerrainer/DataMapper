//! Default security response headers.
//!
//! Adopts FLEET-STRONGHOLDS §5.1 — the cross-service pattern for
//! browser-side defense-in-depth. Every response, including error
//! paths, carries five headers that harden the browser rendering
//! surface regardless of whether the caller is a browser, curl, or
//! an internal service. Handler-set values take precedence, so a
//! response that legitimately needs a different CSP (there are none
//! today, but future-proof) can override without touching this
//! middleware.
//!
//! Choice of headers matches TIM (the pioneering service in the
//! fleet):
//!
//! - **Content-Security-Policy**: `default-src 'none'; frame-ancestors 'none'`.
//!   DataMapper does not serve HTML app UIs — templates that
//!   render as JSON never load subresources, and templates that
//!   accidentally render as HTML (see h2ck.me v1 M2 / N1) also
//!   should not load anything.
//! - **Strict-Transport-Security**: `max-age=63072000; includeSubDomains; preload`.
//!   Two-year HSTS with preload — matches the fleet default.
//!   Applied to non-TLS responses too because a reverse proxy in
//!   front of us terminates TLS and the header downstream still
//!   flags the connection as HSTS-worthy.
//! - **X-Frame-Options**: `DENY`. Redundant with the CSP
//!   `frame-ancestors 'none'` but retained for older browsers
//!   (IE, older Safari).
//! - **X-Content-Type-Options**: `nosniff`. Kills the MIME-sniff
//!   fallback lane completely — DataMapper's response
//!   `Content-Type` is authoritative.
//! - **Referrer-Policy**: `no-referrer`. Templates that render
//!   HTML never leak the referring URL to another origin.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

/// Wire `security_headers` via
/// `Router::layer(axum::middleware::from_fn(security_headers))`.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let h = response.headers_mut();

    // `.entry(...).or_insert(...)` semantics — never clobber a
    // handler-set value. Today every handler produces JSON without
    // touching these headers, but the guarantee is future-proof.
    for (name, value) in HEADERS.iter() {
        if !h.contains_key(*name) {
            h.insert(*name, HeaderValue::from_static(value));
        }
    }
    response
}

/// The five defaults. Kept as a `const` slice so a test can pin the
/// exact list (rather than each header being asserted individually
/// against a magic constant string).
pub const HEADERS: &[(&str, &str)] = &[
    (
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'",
    ),
    (
        "strict-transport-security",
        "max-age=63072000; includeSubDomains; preload",
    ),
    ("x-frame-options", "DENY"),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
];
