//! REFACTO-REQUIREMENTS §4.3 — cross-implementation reproduction.
//!
//! For every subsystem the Rust target claims to preserve, run the
//! exact same request against BOTH the JS source of truth AND the
//! Rust target, then assert the responses match modulo the
//! divergences catalogued in `DIVERGENCES.md`.
//!
//! This test needs a working Node.js runtime + the JS server staged
//! under `compat/js-server/` with `node_modules/` populated. When
//! either is missing the test skips loudly (not silently) so CI can
//! decide whether to gate on it.
//!
//! CI toggles:
//! * `DATAMAPPER_REPRO_STRICT=1` — fail (rather than skip) when
//!   the JS side can't be reached.
//! * `DATAMAPPER_JS_SERVER_DIR` — override the staged path.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use datamapper::{
    renderer::Renderer,
    router::{self, AppState},
};
use std::sync::Arc;
use tempfile::TempDir;

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn js_server_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DATAMAPPER_JS_SERVER_DIR") {
        return Some(PathBuf::from(p));
    }
    let default = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compat/js-server");
    if default.join("node_modules").exists() && default.join("server.js").exists() {
        Some(default)
    } else {
        None
    }
}

fn strict() -> bool {
    std::env::var("DATAMAPPER_REPRO_STRICT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn spawn_js(dir: &std::path::Path, port: u16) -> Result<Child, std::io::Error> {
    let mut cmd = Command::new("node");
    cmd.arg("server.js")
        .current_dir(dir)
        .env("PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    // Wait for the JS boot line before returning.
    let stdout = child.stdout.take().expect("stdout piped");
    let reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(10);
    for line in reader.lines() {
        if Instant::now() > deadline {
            break;
        }
        let line = line.unwrap_or_default();
        if line.starts_with("DataMapper listening on :") {
            return Ok(child);
        }
    }
    // Boot didn't complete cleanly — kill and error.
    let _ = child.kill();
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "JS server did not emit boot line within 10s",
    ))
}

async fn spawn_rust(dsl_root: &std::path::Path) -> String {
    let state = AppState {
        renderer: Arc::new(Renderer::new(dsl_root.to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

/// Fields the JS server unavoidably leaks into `this` (express
/// internals + the render options). These are the D-015 divergence
/// keys. Two-sided equivalence is asserted modulo these.
const JS_CONTEXT_LEAK_KEYS: &[&str] = &["settings", "layout", "_locals", "cache"];

/// Recursively remove the JS-side leak keys from any object nested
/// under `value`.
fn strip_leak_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for k in JS_CONTEXT_LEAK_KEYS {
                map.remove(*k);
            }
            for (_, v) in map.iter_mut() {
                strip_leak_keys(v);
            }
        }
        Value::Array(a) => {
            for v in a.iter_mut() {
                strip_leak_keys(v);
            }
        }
        _ => {}
    }
}

/// Keys the JS `/healthz` and Rust `/healthz` differ on: `ts` is a
/// wall-clock timestamp, `project` is an incidental JS field.
fn strip_healthz_volatile(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("ts");
        map.remove("project");
    }
}

#[tokio::test]
async fn cross_impl_repro_matches_modulo_documented_divergences() {
    let js_dir = match js_server_dir() {
        Some(d) => d,
        None => {
            let msg = "cross-impl repro skipped — JS server not staged at compat/js-server/ (see compat/README.md)";
            if strict() {
                panic!("DATAMAPPER_REPRO_STRICT=1 but {msg}");
            }
            eprintln!("{msg}");
            return;
        }
    };

    // Repopulate the JS-side DSL tree from the git-tracked corpus
    // so the JS server has templates to serve. Both paths live
    // under `compat/`; the DSL tree under `compat/js-server/DSL/`
    // is intentionally gitignored so the two never drift.
    let corpus = js_dir.join("DSL");
    let source_dsl = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compat/js-DSL");
    if corpus.exists() {
        std::fs::remove_dir_all(&corpus).ok();
    }
    copy_dir(&source_dsl, &corpus);
    assert!(
        corpus.exists(),
        "expected JS DSL corpus at {}",
        corpus.display()
    );

    let js_port = free_port();
    let mut js = match spawn_js(&js_dir, js_port) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("cross-impl repro skipped — JS server failed to boot: {e}");
            if strict() {
                panic!("DATAMAPPER_REPRO_STRICT=1 but {msg}");
            }
            eprintln!("{msg}");
            return;
        }
    };

    // Stage the same DSL tree under Rust.
    let tmp = TempDir::new().unwrap();
    copy_dir(&corpus, tmp.path());
    let rust_base = spawn_rust(tmp.path()).await;
    let js_base = format!("http://127.0.0.1:{}", js_port);

    let client = reqwest::Client::new();

    // Fixture set: exact same JSON body sent to both impls; assert
    // the parsed JSON responses equal after stripping documented
    // divergence keys.
    let cases: &[(&str, Value)] = &[
        (
            "samples/arrays/map_products",
            json!({"products":[
                {"sku":"A1","name":"Widget","price":19.9},
                {"sku":"B2","name":"Gadget","price":29.5}
            ]}),
        ),
        (
            "samples/objects/select_fields",
            json!({"user":{"id":10,"first":"Ava","last":"Stone"},"role":"admin"}),
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

    let mut failures: Vec<String> = Vec::new();

    for (path, body) in cases {
        let js_resp = client
            .post(format!("{js_base}/{path}"))
            .header("content-type", "application/json")
            .header("type", "json")
            .body(body.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("JS HTTP failed for {path}: {e}"));
        let rust_resp = client
            .post(format!("{rust_base}/{path}"))
            .header("content-type", "application/json")
            .header("type", "json")
            .body(body.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("Rust HTTP failed for {path}: {e}"));

        if js_resp.status() != rust_resp.status() {
            failures.push(format!(
                "{path}: status mismatch — JS {} vs Rust {}",
                js_resp.status(),
                rust_resp.status()
            ));
            continue;
        }
        let mut js_body: Value = js_resp.json().await.unwrap();
        let mut rust_body: Value = rust_resp.json().await.unwrap();
        strip_leak_keys(&mut js_body);
        strip_leak_keys(&mut rust_body);
        if js_body != rust_body {
            failures.push(format!(
                "{path}: JSON differ — JS={} vs Rust={}",
                js_body, rust_body
            ));
        }
    }

    // /healthz separately — both should emit {service,ok,ts}; the JS
    // one also emits `project` (a leftover from an earlier design).
    // Strip `ts` (wall-clock) + `project` (JS-only) and compare.
    let js_h = client
        .get(format!("{js_base}/healthz"))
        .send()
        .await
        .unwrap();
    let rust_h = client
        .get(format!("{rust_base}/healthz"))
        .send()
        .await
        .unwrap();
    if js_h.status() != rust_h.status() {
        failures.push(format!(
            "/healthz: status mismatch JS {} vs Rust {}",
            js_h.status(),
            rust_h.status()
        ));
    }
    let mut js_h_body: Value = js_h.json().await.unwrap();
    let mut rust_h_body: Value = rust_h.json().await.unwrap();
    strip_healthz_volatile(&mut js_h_body);
    strip_healthz_volatile(&mut rust_h_body);
    if js_h_body != rust_h_body {
        failures.push(format!(
            "/healthz: JS={} vs Rust={}",
            js_h_body, rust_h_body
        ));
    }

    // Best-effort JS teardown.
    let _ = js.kill();
    let _ = js.wait();

    if !failures.is_empty() {
        panic!(
            "cross-impl repro failures ({} total) — either fix Rust to match JS or add a DIVERGENCES.md entry:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
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
