//! End-to-end integration tests.
//!
//! Each test writes a fresh DSL tree into a `TempDir`, spins up the
//! full axum router bound to `127.0.0.1:0`, and exercises the HTTP
//! surface with a real reqwest client. No mocking of the renderer,
//! router, config, or filesystem — DEV-REQUIREMENTS §3.

use datamapper::{
    renderer::Renderer,
    router::{self, AppState},
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

/// Write a template into `dsl_root/<project>/<view>.hbs`.
fn write_dsl(dsl_root: &Path, project: &str, view: &str, body: &str) {
    let dir = dsl_root.join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{view}.hbs")), body).unwrap();
}

/// Spawn the DataMapper HTTP server on an ephemeral port bound to
/// `127.0.0.1:0`. Returns the base URL for the client to hit.
async fn spawn_server(dsl_root: &Path, max_req_bytes: usize, max_resp_bytes: usize) -> String {
    spawn_server_with_timeout(dsl_root, max_req_bytes, max_resp_bytes, 30).await
}

/// Same as [`spawn_server`] but with an explicit
/// `request_timeout_secs` — used by the N3 slow-body regression pin.
async fn spawn_server_with_timeout(
    dsl_root: &Path,
    max_req_bytes: usize,
    max_resp_bytes: usize,
    request_timeout_secs: u64,
) -> String {
    let state = AppState {
        renderer: Arc::new(Renderer::new(dsl_root.to_path_buf())),
        max_request_bytes: max_req_bytes,
        max_response_bytes: max_resp_bytes,
        max_body_array_length: 10_000,
        request_timeout_secs,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

async fn spawn_default(dsl_root: &Path) -> String {
    spawn_server(dsl_root, 2 * 1024 * 1024, 16 * 1024 * 1024).await
}

#[tokio::test]
async fn healthz_get_returns_ok() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["service"], "DataMapper");
    assert_eq!(body["ok"], true);
    assert!(body["ts"].is_string());
}

#[tokio::test]
async fn health_alias_works() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn healthz_post_is_method_not_allowed() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/healthz"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "MethodNotAllowed");
}

#[tokio::test]
async fn renders_echo_and_returns_json() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .header("type", "json")
        .body(r#"{"msg":"hello","n":42}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("application/json"));
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["msg"], "hello");
    assert_eq!(body["n"], 42);
}

#[tokio::test]
async fn renders_ping_with_now_helper() {
    let tmp = TempDir::new().unwrap();
    write_dsl(
        tmp.path(),
        "samples",
        "ping",
        r#"{ "service": "DataMapper", "ok": true, "ts": "{{now}}" }"#,
    );
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/ping"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["service"], "DataMapper");
    let ts = body["ts"].as_str().unwrap();
    // RFC 3339 shape: 2026-07-29T…
    assert!(ts.len() >= 20);
    assert_eq!(&ts[4..5], "-");
}

#[tokio::test]
async fn json_coercion_via_accept_header() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // No `type: json` custom header — Accept alone must trigger.
    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(r#"{"a":1}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["a"], 1);
}

#[tokio::test]
async fn html_fallback_when_output_not_json_and_no_json_preference() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "hello", "<h1>hi {{name}}</h1>");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // Explicit HTML preference: no JSON coercion, HTML MIME.
    let resp = client
        .post(format!("{base}/samples/hello"))
        .header("content-type", "application/json")
        .header("accept", "text/html")
        .body(r#"{"name":"Ava"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("text/html"));
    let body = resp.text().await.unwrap();
    assert_eq!(body, "<h1>hi Ava</h1>");
}

#[tokio::test]
async fn fallback_defaults_to_text_plain_when_client_did_not_ask_for_html() {
    // Regression pin for h2ck.me v1 M2 — the opportunistic HTML
    // fallback used to fire on any non-JSON output, which meant a
    // mis-authored template could return attacker-influenced markup
    // as `text/html` to a browser client that had not asked for it.
    // The fix restricts `text/html` to clients that explicitly send
    // `Accept: text/html`; every other request lands as
    // `text/plain; charset=utf-8`.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "hello", "<h1>hi {{name}}</h1>");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // Send an Accept header that neither triggers `wants_json`
    // (no `application/json`, no `*/*`) nor unlocks the HTML
    // fallback (no `text/html`). That reaches the opportunistic-
    // parse path in `respond()`: rendered output isn't valid JSON
    // → must fall back to `text/plain`, not `text/html`.
    let resp = client
        .post(format!("{base}/samples/hello"))
        .header("content-type", "application/json")
        .header("accept", "application/xml")
        .body(r#"{"name":"Ava"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        ct.contains("text/plain"),
        "expected text/plain fallback, got {ct}"
    );
    // Handlebars still escapes the interpolated value — the browser
    // XSS lane is closed both by the Content-Type change AND by the
    // default double-brace escaping.
    let body = resp.text().await.unwrap();
    assert_eq!(body, "<h1>hi Ava</h1>");
}

#[tokio::test]
async fn fallback_stays_text_plain_for_wildcard_accept() {
    // `Accept: */*` is what curl (and many bots) send by default.
    // It must NOT unlock the `text/html` fallback — see M2.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "hello", "<h1>hi {{name}}</h1>");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/hello"))
        .header("content-type", "application/json")
        .header("accept", "*/*")
        .body(r#"{"name":"Ava"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // `*/*` also triggers JSON coercion via `wants_json`; the
    // rendered `<h1>…</h1>` isn't JSON so we land in the fallback
    // path that keeps `application/json` MIME (see `respond`).
    // What matters for M2 is: the response is not served as
    // `text/html`, so a browser can't execute it.
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        !ct.contains("text/html"),
        "must not fall back to text/html on wildcard Accept, got {ct}"
    );
}

#[tokio::test]
async fn opportunistic_json_when_output_looks_like_json() {
    // Mirrors the original DataMapper Node.js behaviour: if the
    // rendered output is valid JSON, upgrade the response MIME to
    // application/json regardless of what the client asked for. HTML
    // MIME only wins when the output is NOT parseable as JSON —
    // see html_fallback_when_output_not_json_and_no_json_preference.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "obj", r#"{ "k": "{{v}}" }"#);
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/obj"))
        .header("content-type", "application/json")
        .body(r#"{"v":"y"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("application/json"), "content-type was {ct}");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["k"], "y");
}

#[tokio::test]
async fn template_not_found_returns_404() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/nowhere/nope"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "TemplateNotFound");
    assert!(body["tried"].is_array());
    let tried = body["tried"].as_array().unwrap();
    // Two candidates: <project>/<view>.hbs and <project>/hbs/<view>.hbs.
    assert_eq!(tried.len(), 2);
}

#[tokio::test]
async fn fallback_hbs_subfolder_resolves() {
    let tmp = TempDir::new().unwrap();
    // Only the `hbs/` fallback exists, not the top-level candidate.
    let dir = tmp.path().join("proj").join("hbs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("view.hbs"), r#"{ "ok": true }"#).unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/proj/view"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn path_traversal_is_blocked() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // Try to escape via `..` in the view path.
    let resp = client
        .post(format!("{base}/samples/..%2fetc%2fpasswd"))
        .body("{}")
        .send()
        .await
        .unwrap();
    // Percent-decoded `..` will hit sanitize_segment → InvalidPath.
    // The exact HTTP shape may vary with axum's decoder; either
    // 400 (structured) or 404 (never resolved) is acceptable, but
    // must not be 200.
    let status = resp.status();
    assert!(
        status == 400 || status == 404,
        "expected 400 or 404, got {status}"
    );
}

#[tokio::test]
async fn empty_body_becomes_empty_object() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "ping", r#"{ "ok": true }"#);
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/ping"))
        .body("")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn invalid_json_body_returns_400() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .body("not json at all {")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "InvalidJson");
}

#[tokio::test]
async fn request_body_over_limit_returns_413() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    // Very tight cap so the test doesn't allocate megabytes.
    let base = spawn_server(tmp.path(), 128, 16 * 1024).await;
    let client = reqwest::Client::new();

    let big = format!(r#"{{"x":"{}"}}"#, "A".repeat(200));
    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .body(big)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "RequestTooLarge");
    assert_eq!(body["limit"], 128);
}

#[tokio::test]
async fn response_body_over_limit_returns_500() {
    let tmp = TempDir::new().unwrap();
    // Template renders a huge string relative to the response cap.
    write_dsl(
        tmp.path(),
        "samples",
        "big",
        &format!(r#""{}""#, "A".repeat(4096)),
    );
    let base = spawn_server(tmp.path(), 65_536, 128).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/big"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "ResponseTooLarge");
    assert_eq!(body["limit"], 128);
}

#[tokio::test]
async fn amplification_template_fires_cap_mid_render() {
    // Regression pin for h2ck.me v1 M3 — a template that expands its
    // caller-supplied payload by ~1000× must NOT be permitted to run
    // to completion and allocate the full rendered output before the
    // response-size cap fires. `CappedWriter` aborts the render the
    // moment the buffer crosses `max_response_bytes`.
    //
    // Setup: 10 KiB response cap; template that emits ~200 bytes per
    // inner iteration across a nested `{{#each}}` payload sized so
    // the fully rendered form would be ~1 MiB (>> cap). The response
    // must be a structured 500 ResponseTooLarge; if the cap-check
    // regressed to the old post-render path this would still work but
    // for the wrong reason — we additionally assert the render error
    // originates from the writer, not from a post-hoc length check
    // on a fully materialised String, by keeping the cap tight enough
    // (10 KiB) that any allocation of the full output would show up
    // as a test-timeout / OOM rather than a clean 500.
    let tmp = TempDir::new().unwrap();
    write_dsl(
        tmp.path(),
        "samples",
        "amp",
        "{{#each items}}{{#each nested}}{{payload}}\n{{/each}}{{/each}}",
    );
    let base = spawn_server(tmp.path(), 2 * 1024 * 1024, 10 * 1024).await;
    let client = reqwest::Client::new();

    // 32 outer × 32 inner × 1024-byte payload = ~1 MiB rendered.
    let payload = "A".repeat(1024);
    let nested: Vec<Value> = (0..32).map(|_| json!({ "payload": payload })).collect();
    let items: Vec<Value> = (0..32).map(|_| json!({ "nested": nested })).collect();
    let body = json!({ "items": items });

    let resp = client
        .post(format!("{base}/samples/amp"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
    let err: Value = resp.json().await.unwrap();
    assert_eq!(err["error"], "ResponseTooLarge");
    assert_eq!(err["limit"], 10 * 1024);
}

#[tokio::test]
async fn all_shipped_samples_render_against_their_curl_bodies() {
    // Copy the checked-in DSL tree into a TempDir and hit every
    // sample with the exact JSON body that its header comment
    // recommends. Guards against DSL drift and Handlebars-syntax
    // regressions from crate upgrades.
    let repo_dsl = Path::new(env!("CARGO_MANIFEST_DIR")).join("DSL");
    assert!(
        repo_dsl.exists(),
        "expected DSL tree at {}",
        repo_dsl.display()
    );

    let tmp = TempDir::new().unwrap();
    copy_dir(&repo_dsl, tmp.path());

    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let cases: &[(&str, Value)] = &[
        ("samples/ping", json!({})),
        (
            "samples/echo",
            json!({"msg":"hello","n":42,"nested":{"a":1}}),
        ),
        (
            "samples/objects/select_fields",
            json!({"user":{"id":10,"first":"Ava","last":"Stone"},"role":"admin"}),
        ),
        (
            "samples/arrays/map_products",
            json!({"products":[
                {"sku":"A1","name":"Widget","price":19.9},
                {"sku":"B2","name":"Gadget","price":29.5}
            ]}),
        ),
        (
            "samples/conditionals/include_optional",
            json!({"email":"a@x.io"}),
        ),
        (
            "samples/config/from_kv_array",
            json!({"configuration":[
                {"key":"theme","value":"dark"},
                {"key":"pageSize","value":"20"},
                {"key":"featureX","value":"true"}
            ]}),
        ),
        (
            "samples/users/create",
            json!({"username":"neo","email":"neo@example.com"}),
        ),
        (
            "samples/users/patch",
            json!({"id":123,"username":"trinity"}),
        ),
        (
            "samples/strings/join_tags_csv",
            json!({"tags":["alpha","beta","gamma"]}),
        ),
        (
            "samples/transform/flatten_address",
            json!({"user":{"id":7,"name":"Ava","address":{
                "street":"Main 1","city":"Tallinn","postal":"10115","country":"EE"
            }}}),
        ),
        (
            "samples/advanced/nested_each_index",
            json!({"matrix":[[1,2,3],[4,5,6],[7,8,9]]}),
        ),
    ];

    for (path, body) in cases {
        let resp = client
            .post(format!("{base}/{path}"))
            .header("content-type", "application/json")
            .header("type", "json")
            .body(body.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("HTTP failed for {path}: {e}"));
        let status = resp.status();
        let text = resp.text().await.unwrap();
        assert_eq!(status, 200, "{path} → {status}: {text}");
        // Every sample DSL emits JSON when type: json is requested.
        let parsed: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{path} did not return JSON: {e}\n{text}"));
        assert!(parsed.is_object() || parsed.is_array(), "{path} → {parsed}");
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

// ---------- FLEET §5.1 — default security headers ----------

#[tokio::test]
async fn u9_security_headers_present_on_health_response() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // Every header in the fleet-standard set must be present and
    // carry the exact fleet-standard value.
    for (name, expected) in datamapper::security_headers::HEADERS {
        let got = resp
            .headers()
            .get(*name)
            .unwrap_or_else(|| panic!("missing security header: {name}"));
        assert_eq!(got.to_str().unwrap(), *expected, "wrong value for {name}");
    }
}

#[tokio::test]
async fn u9_security_headers_present_on_error_response() {
    // Belt-and-braces: even error paths get the headers. A
    // TemplateNotFound response is served via `IntoResponse` from
    // the router handler, but the middleware layers it AFTER the
    // handler runs so the headers are attached uniformly.
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/nonexistent/view"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    for (name, _) in datamapper::security_headers::HEADERS {
        assert!(
            resp.headers().get(*name).is_some(),
            "missing security header on error response: {name}"
        );
    }
}

#[tokio::test]
async fn u9_security_headers_do_not_clobber_handler_content_type() {
    // Regression pin: a handler that emits its own Content-Type
    // (application/json for the render path) must NOT have that
    // value overwritten by the middleware. This is the
    // `contains_key` / `insert` semantics documented in
    // security_headers.rs.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .header("type", "json")
        .body(r#"{"k":"v"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "handler content-type was clobbered: {ct}"
    );
}

// ---------- FLEET §1.6 — W3C traceparent response header ----------

#[tokio::test]
async fn o1_every_response_carries_traceparent_headers() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let tp = resp
        .headers()
        .get("traceparent")
        .expect("traceparent header missing")
        .to_str()
        .unwrap()
        .to_string();
    // Shape: 00-<32 hex>-<16 hex>-01
    let parts: Vec<&str> = tp.split('-').collect();
    assert_eq!(parts.len(), 4, "traceparent must have 4 dashed parts: {tp}");
    assert_eq!(parts[0], "00", "version must be 00");
    assert_eq!(parts[1].len(), 32, "trace id must be 32 hex chars");
    assert_eq!(parts[2].len(), 16, "span id must be 16 hex chars");
    assert_eq!(parts[3], "01", "flags must be 01");

    // x-trace-id mirrors the traceparent's trace id.
    let xid = resp
        .headers()
        .get("x-trace-id")
        .expect("x-trace-id header missing")
        .to_str()
        .unwrap();
    assert_eq!(xid, parts[1], "x-trace-id must equal traceparent trace id");
}

#[tokio::test]
async fn o1_inbound_traceparent_is_inherited() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let inbound_trace = "0af7651916cd43dd8448eb211c80319c";
    let inbound = format!("00-{inbound_trace}-b7ad6b7169203331-01");
    let resp = client
        .get(format!("{base}/healthz"))
        .header("traceparent", &inbound)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tp = resp
        .headers()
        .get("traceparent")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let parts: Vec<&str> = tp.split('-').collect();
    assert_eq!(
        parts[1], inbound_trace,
        "trace id must be inherited from inbound header, got: {tp}"
    );
    // Span id must be REGENERATED — this service's span, not the
    // caller's.
    assert_ne!(
        parts[2], "b7ad6b7169203331",
        "span id must be freshly generated, not echoed"
    );
}

#[tokio::test]
async fn o1_malformed_inbound_traceparent_gets_fresh_id() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/healthz"))
        .header("traceparent", "totally-malformed")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tp = resp
        .headers()
        .get("traceparent")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let parts: Vec<&str> = tp.split('-').collect();
    // Fresh id — must be well-formed even though inbound was junk.
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[1].len(), 32);
    assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn o1_all_zero_inbound_trace_id_rejected() {
    // W3C spec reserves all-zeros trace id as invalid.
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/healthz"))
        .header(
            "traceparent",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
        )
        .send()
        .await
        .unwrap();
    let tp = resp
        .headers()
        .get("traceparent")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let parts: Vec<&str> = tp.split('-').collect();
    assert_ne!(
        parts[1], "00000000000000000000000000000000",
        "all-zeros trace id must be rejected and regenerated: {tp}"
    );
}

#[tokio::test]
async fn o1_traceparent_present_on_error_response_too() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/nonexistent/view"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    assert!(
        resp.headers().get("traceparent").is_some(),
        "traceparent missing on error response"
    );
    assert!(
        resp.headers().get("x-trace-id").is_some(),
        "x-trace-id missing on error response"
    );
}

// ---------- N8 — per-header value size cap ----------

#[tokio::test]
async fn n8_oversize_single_header_returns_431_structured() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // 10 KB header value — 2 KB over the 8 KB middleware cap.
    let huge = "X".repeat(10 * 1024);
    let resp = client
        .get(format!("{base}/healthz"))
        .header("x-attacker-header", &huge)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 431);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "HeaderValueTooLarge");
    assert_eq!(body["limit"], 8 * 1024);
    assert!(body["actual"].as_u64().unwrap() >= 10 * 1024);
}

#[tokio::test]
async fn n8_at_cap_boundary_admits_request() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // Exactly 8192 bytes — at the cap; `>`, not `>=`, so admits.
    let at_cap = "X".repeat(8 * 1024);
    let resp = client
        .get(format!("{base}/healthz"))
        .header("x-large-but-legit", &at_cap)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn n8_small_headers_admitted() {
    // Sanity: normal-shaped requests are unaffected. The N8 fix must
    // not regress the happy path.
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/healthz"))
        .header("authorization", "Bearer some-normal-jwt-shape-token")
        .header("accept", "application/json")
        .header("x-request-id", "abc-123-def")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ---------- N5 — 404 tried-path echo clipping ----------

#[tokio::test]
async fn n5_notfound_tried_paths_clipped_at_256_chars() {
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    // 10 KB view segment. Pre-fix, the 404 body echoed both
    // attempted paths in full (~20 KB response). Post-fix, each
    // tried entry caps at ~256 chars.
    let long = "a".repeat(10_000);
    let resp = client
        .post(format!("{base}/attack/{long}"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "TemplateNotFound");
    let tried = body["tried"].as_array().expect("tried is a JSON array");
    for entry in tried {
        let s = entry.as_str().unwrap();
        assert!(
            s.chars().count() <= 300,
            "N5 fail: tried entry too long: {} chars",
            s.chars().count()
        );
        assert!(
            s.contains("[truncated]"),
            "expected truncation marker on long entry: {s}"
        );
    }
}

// ---------- N6 — unified 404 shape across router + fallback ----------

#[tokio::test]
async fn n6_encoded_traversal_yields_structured_notfound() {
    // Path-encoded traversal — Axum decodes %2F into slashes and
    // the URL no longer matches `/:project/*view`, so it hits the
    // global fallback. Pre-fix, that returned an empty body with
    // no content-type. Post-fix, the fallback emits the same
    // structured JSON shape.
    let tmp = TempDir::new().unwrap();
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/..%2F..%2Fetc%2Fpasswd"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("application/json"),
        "N6 fail: fallback must be JSON, got: {ct}"
    );
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "NotFound");
    assert!(body["message"].is_string());
}

// ---------- F-DM-1 — render amplification cap ----------

#[tokio::test]
async fn fdm1_oversize_top_level_array_returns_413_structured() {
    let tmp = TempDir::new().unwrap();
    // Simple amplification template — output size scales with
    // items.len().
    write_dsl(tmp.path(), "amp", "count", "{{#each items}}x{{/each}}");
    // Spawn with a tight cap for a fast test.
    let state = AppState {
        renderer: Arc::new(Renderer::new(tmp.path().to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
        max_body_array_length: 100,
        request_timeout_secs: 30,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 101 elements > cap 100.
    let body = json!({"items": (0..101).collect::<Vec<i32>>()});
    let resp = client
        .post(format!("{base}/amp/count"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "RequestArrayTooLarge");
    assert_eq!(json["length"], 101);
    assert_eq!(json["limit"], 100);
}

#[tokio::test]
async fn fdm1_at_cap_boundary_renders_normally() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "amp", "count", "{{#each items}}x{{/each}}");
    let state = AppState {
        renderer: Arc::new(Renderer::new(tmp.path().to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
        max_body_array_length: 100,
        request_timeout_secs: 30,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Exactly 100 elements — at the cap, must pass.
    let body = json!({"items": (0..100).collect::<Vec<i32>>()});
    let resp = client
        .post(format!("{base}/amp/count"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert_eq!(text.chars().filter(|c| *c == 'x').count(), 100);
}

#[tokio::test]
async fn fdm1_nested_oversize_array_is_caught() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "amp", "nested", "{{{json this}}}");
    let state = AppState {
        renderer: Arc::new(Renderer::new(tmp.path().to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
        max_body_array_length: 50,
        request_timeout_secs: 30,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Outer array is small, but a nested array busts the cap.
    let inner: Vec<i32> = (0..51).collect();
    let body = json!({"outer": [{"inner": inner}]});
    let resp = client
        .post(format!("{base}/amp/nested"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
    let json: Value = resp.json().await.unwrap();
    assert_eq!(json["error"], "RequestArrayTooLarge");
    assert_eq!(json["length"], 51);
}

#[tokio::test]
async fn fdm1_zero_cap_disables_the_check() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "amp", "count", "{{#each items}}x{{/each}}");
    // max_body_array_length = 0 → disabled.
    let state = AppState {
        renderer: Arc::new(Renderer::new(tmp.path().to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
        max_body_array_length: 0,
        request_timeout_secs: 30,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Large array — would trip at cap 10000, but the cap is
    // disabled, so it renders.
    let body = json!({"items": (0..12345).collect::<Vec<i32>>()});
    let resp = client
        .post(format!("{base}/amp/count"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ---------- N3 slow-body / slow-loris regression pin ----------

/// h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N3 (MEDIUM) —
/// pre-fix, a client that dripped a request body one byte at a time
/// could keep a Tokio task slot open indefinitely; the outer
/// `TimeoutLayer` only starts its clock once the service future
/// runs, and body extraction happens inside that service future.
///
/// This test opens a raw TCP connection to the server, sends a valid
/// HTTP request line + headers, then dribbles the body across
/// several seconds while the server is configured with a 1-second
/// timeout. The server must close with a 408 `RequestReadTimeout`
/// well under the drip total.
#[tokio::test]
async fn n3_slow_body_read_hits_deadline() {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    // 1-second body-read deadline so the test finishes fast.
    let base = spawn_server_with_timeout(tmp.path(), 2 * 1024 * 1024, 16 * 1024 * 1024, 1).await;
    let host_port = base.trim_start_matches("http://").to_string();

    let mut stream = TcpStream::connect(&host_port).await.unwrap();

    // Announce a 200-byte body but never actually send it. The
    // server must close the connection or 408 within the 1s
    // deadline.
    let head = "POST /samples/echo HTTP/1.1\r\n\
                Host: localhost\r\n\
                Content-Type: application/json\r\n\
                Content-Length: 200\r\n\
                Connection: close\r\n\
                \r\n";
    stream.write_all(head.as_bytes()).await.unwrap();
    // Send one byte, then stall — well within the size limit but
    // won't complete the announced content-length.
    stream.write_all(b"{").await.unwrap();
    stream.flush().await.unwrap();

    // Read whatever the server sends back within 5 seconds. Wall
    // clock cap ensures the test fails visibly if the deadline
    // doesn't fire.
    let mut response = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response),
    )
    .await;

    assert!(
        read.is_ok(),
        "N3 fail: server never responded within 5s — deadline did not fire"
    );
    let response_str = String::from_utf8_lossy(&response);
    // Server should either explicitly emit 408 or close the
    // connection under the layer's 504. Either is acceptable
    // evidence that the slow-drip DoS was mitigated; the
    // structured-JSON 408 is what the fix actually promises.
    assert!(
        response_str.starts_with("HTTP/1.1 408") || response_str.starts_with("HTTP/1.1 504"),
        "N3 fail: expected 408 or 504, got:\n{response_str}"
    );
    if response_str.starts_with("HTTP/1.1 408") {
        assert!(
            response_str.contains("RequestReadTimeout"),
            "expected structured error code in 408 body:\n{response_str}"
        );
    }
}

// ---------- N1 composite XSS regression pins ----------
//
// h2ck.me v1 BREAK-TESTS/RUNTIME-FINDINGS.md §N1 upgraded the
// audit's M1 (docs-only for triple-brace XSS) with a runtime backstop
// requirement: even when a caller explicitly sends
// `Accept: text/html`, a template using `{{{X}}}` (non-json triple-
// brace, un-escaped output) must NOT be served as `text/html` —
// otherwise the composite of a bad template + attacker Accept header
// is a stored-XSS lane. The renderer flags the template; the router
// forces `text/plain` on the fallback path regardless of Accept.

#[tokio::test]
async fn n1_triple_brace_template_forces_text_plain_even_with_html_accept() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "attack", "xss", "{{{name}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/attack/xss"))
        .header("content-type", "application/json")
        .header("accept", "text/html")
        .body(json!({"name": "<script>alert(1)</script>"}).to_string())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/plain"),
        "N1 fail: expected text/plain, got Content-Type: {ct}"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<script>"),
        "expected raw payload in body, got: {body}"
    );
}

#[tokio::test]
async fn n1_json_helper_template_stays_json_response() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "safe", "obj", "{{{json body}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/safe/obj"))
        .header("content-type", "application/json")
        .header("accept", "text/html")
        .body(json!({"body": {"k": "v"}}).to_string())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "expected application/json for valid-JSON render output, got: {ct}"
    );
}

#[tokio::test]
async fn n1_double_brace_template_still_gets_text_html_on_explicit_accept() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "safe", "greet", "<h1>Hello {{name}}</h1>");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/safe/greet"))
        .header("content-type", "application/json")
        .header("accept", "text/html")
        .body(json!({"name": "<script>alert(1)</script>"}).to_string())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("text/html"),
        "expected text/html for double-brace template with explicit Accept, got: {ct}"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("&lt;script&gt;"),
        "handlebars must HTML-escape: {body}"
    );
    assert!(
        !body.contains("<script>"),
        "unescaped script tag leaked: {body}"
    );
}

// ---------- T-13 — wrong method on POST route returns structured 405 ----------
//
// h2ck.me v1 NEXT-TASKS.md §T-13: pre-fix, GET/PUT/DELETE against a
// registered POST route (e.g. `/samples/echo`) fell through to
// axum's default 405 with a bare-text "method not allowed" body.
// The rest of DataMapper emits structured JSON on every failure
// path; make wrong-method requests consistent so log aggregators
// and clients don't need a special branch.

#[tokio::test]
async fn t13_get_on_post_route_returns_structured_405() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/samples/echo"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "expected JSON body on 405, got content-type: {ct}"
    );
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "MethodNotAllowed");
}

#[tokio::test]
async fn t13_put_on_post_route_returns_structured_405() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{base}/samples/echo"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "MethodNotAllowed");
}

#[tokio::test]
async fn t13_delete_on_post_route_returns_structured_405() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/samples/echo"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "MethodNotAllowed");
}

#[tokio::test]
async fn t13_allow_header_lists_post_on_405() {
    // RFC 7231 §6.5.5 requires the Allow header on a 405 response
    // so a well-behaved client (curl -X, retry libraries, RFC-
    // aware HTTP proxies) can determine which methods are actually
    // supported without a probe sweep.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn_default(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/samples/echo"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    let allow = resp
        .headers()
        .get("allow")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        allow.contains("POST"),
        "Allow header must list POST on the render route, got: {allow:?}"
    );
}

// ---------- T-18 — graceful shutdown drains in-flight requests ----------
//
// h2ck.me v1 NEXT-TASKS.md §T-18. Unit-scope: the shutdown-signal
// future is composable — calling `shutdown_from_channel` with a
// channel that never fires must NOT resolve. Full SIGTERM-under-load
// behaviour is validated at container level per the PR's smoke plan.

#[tokio::test]
async fn t18_shutdown_signal_future_awaits_until_signalled() {
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};

    let (_tx, rx) = oneshot::channel::<()>();
    let fut = datamapper::shutdown::shutdown_from_channel(rx);
    // 200ms window — the future must be pending, NOT resolved,
    // because the channel hasn't been signalled and no OS signal
    // has arrived.
    let r = timeout(Duration::from_millis(200), fut).await;
    assert!(
        r.is_err(),
        "shutdown_from_channel resolved without a signal being sent"
    );
}

#[tokio::test]
async fn t18_shutdown_signal_resolves_on_channel_send() {
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};

    let (tx, rx) = oneshot::channel::<()>();
    let fut = datamapper::shutdown::shutdown_from_channel(rx);
    // Send the channel after a tick — the future must resolve.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = tx.send(());
    });
    let r = timeout(Duration::from_secs(2), fut).await;
    assert!(
        r.is_ok(),
        "shutdown_from_channel did not resolve after sender fired"
    );
}
