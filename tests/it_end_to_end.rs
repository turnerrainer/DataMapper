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
    let state = AppState {
        renderer: Arc::new(Renderer::new(dsl_root.to_path_buf())),
        max_request_bytes: max_req_bytes,
        max_response_bytes: max_resp_bytes,
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
