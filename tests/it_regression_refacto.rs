//! Regression tests for REFACTO-REQUIREMENTS §2 fixes.
//!
//! Every test in this file is a fixture that would have FAILED
//! against the code as of `v0.1.0-alpha.1` and is the executable
//! guard for one of the R2 findings in
//! `docs/REFACTO-AUDIT-S2.md`. Removing a test here MUST be paired
//! with an entry in `DIVERGENCES.md` per R4.4.
//!
//! Test names encode the finding they guard: `f01_port_env_var_…`
//! → REFACTO-AUDIT-S2.md F-01.

use datamapper::{
    config::AppConfig,
    renderer::{contains_dot_length_accessor, Renderer},
    router::{self, AppState},
};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn write_dsl(dsl_root: &Path, project: &str, view: &str, body: &str) {
    let dir = dsl_root.join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{view}.hbs")), body).unwrap();
}

async fn spawn(dsl_root: &Path) -> String {
    let state = AppState {
        renderer: Arc::new(Renderer::new(dsl_root.to_path_buf())),
        max_request_bytes: 2 * 1024 * 1024,
        max_response_bytes: 16 * 1024 * 1024,
    };
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

// ---------------------------------------------------------------------------
// F-01 / D-001: `PORT` env var must be honoured when config does not set it.
// ---------------------------------------------------------------------------

/// Serialised guard so PORT env manipulation doesn't race with any
/// other test that reads `std::env`. Also used by F-05 which pokes
/// the filesystem search order.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn f01_port_env_var_is_honoured_when_config_absent() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    // Sanity: no config file present.
    assert!(!tmp.path().join("datamapper.yaml").exists());

    let prev_port = std::env::var("PORT").ok();
    std::env::set_var("PORT", "8181");

    let (cfg, source) = AppConfig::load_or_default().unwrap();
    assert!(source.is_none(), "expected no config file, got {source:?}");
    assert_eq!(cfg.port, 8181, "PORT env var must override default 3000");

    // Restore.
    match prev_port {
        Some(v) => std::env::set_var("PORT", v),
        None => std::env::remove_var("PORT"),
    }
    std::env::set_current_dir(prev_cwd).unwrap();
}

#[test]
fn f01_port_env_var_yields_to_explicit_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    std::fs::write(
        tmp.path().join("datamapper.yaml"),
        "port: 4444\ndsl_path: ./DSL\n",
    )
    .unwrap();

    let prev_port = std::env::var("PORT").ok();
    std::env::set_var("PORT", "9999");

    let (cfg, source) = AppConfig::load_or_default().unwrap();
    assert!(source.is_some());
    assert_eq!(
        cfg.port, 4444,
        "explicit `port:` in config must beat PORT env"
    );

    match prev_port {
        Some(v) => std::env::set_var("PORT", v),
        None => std::env::remove_var("PORT"),
    }
    std::env::set_current_dir(prev_cwd).unwrap();
}

#[test]
fn f01_invalid_port_env_var_falls_back_and_warns() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let prev_port = std::env::var("PORT").ok();
    std::env::set_var("PORT", "not-a-port");

    let (cfg, _) = AppConfig::load_or_default().unwrap();
    assert_eq!(
        cfg.port, 3000,
        "unparseable PORT must not crash — falls back to default"
    );

    match prev_port {
        Some(v) => std::env::set_var("PORT", v),
        None => std::env::remove_var("PORT"),
    }
    std::env::set_current_dir(prev_cwd).unwrap();
}

// ---------------------------------------------------------------------------
// F-02 / D-010: `.length` accessor in a ported JS DSL must warn.
// The unit-level guard is the `contains_dot_length_accessor` detector;
// the wire-level guard is the render still happening (non-strict).
// ---------------------------------------------------------------------------

#[test]
fn f02_detects_dot_length_in_double_brace() {
    assert!(contains_dot_length_accessor("{ \"n\": {{items.length}} }"));
}

#[test]
fn f02_detects_dot_length_in_triple_brace() {
    assert!(contains_dot_length_accessor("{{{ items.length }}}"));
}

#[test]
fn f02_ignores_dot_length_in_hbs_comment() {
    // `{{!-- ... items.length ... --}}` is a Handlebars comment
    // and must not trigger a false-positive warning.
    assert!(!contains_dot_length_accessor(
        "{{!-- example: items.length --}}\n{ \"n\": 0 }"
    ));
}

#[test]
fn f02_ignores_dot_lengthy_identifier() {
    // `.lengthMax` is a different field entirely.
    assert!(!contains_dot_length_accessor("{{ items.lengthMax }}"));
}

#[tokio::test]
async fn f02_ported_js_dsl_with_dot_length_returns_correct_length() {
    // R2.1 option (1): honour input with source-of-truth semantics.
    // A JS DSL using `{{items.length}}` must render the array length,
    // not raise 500, not silently emit empty. The compat rewriter in
    // renderer::rewrite_dot_length handles this transparently.
    let tmp = TempDir::new().unwrap();
    write_dsl(
        tmp.path(),
        "samples",
        "count",
        r#"{ "n": {{#if items}}{{items.length}}{{else}}0{{/if}} }"#,
    );
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/samples/count"))
        .header("content-type", "application/json")
        .body(r#"{"items":[1,2,3,4]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["n"], 4,
        "ported `{{{{items.length}}}}` must render as 4 (JS parity)"
    );
}

#[tokio::test]
async fn f02_ported_js_dsl_dot_length_in_if_block() {
    // The JS `map_products.hbs` idiom:
    // `{{#if products}}{{products.length}}{{else}}0{{/if}}`.
    let tmp = TempDir::new().unwrap();
    write_dsl(
        tmp.path(),
        "samples",
        "count_if",
        r#"{ "n": {{#if products}}{{products.length}}{{else}}0{{/if}} }"#,
    );
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    // With items:
    let resp = client
        .post(format!("{base}/samples/count_if"))
        .header("content-type", "application/json")
        .body(r#"{"products":[{"k":1},{"k":2}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["n"], 2);

    // Without items:
    let resp = client
        .post(format!("{base}/samples/count_if"))
        .header("content-type", "application/json")
        .body(r#"{}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["n"], 0);
}

#[tokio::test]
async fn f02_dot_length_as_if_condition() {
    // JS: `{{#if items.length}}has-items{{else}}empty{{/if}}` —
    // truthiness of `items.length`. The rewriter converts this to
    // `{{#if (len items)}}...{{/if}}`, which handlebars-rust
    // resolves to a numeric length; nonzero is truthy.
    let tmp = TempDir::new().unwrap();
    write_dsl(
        tmp.path(),
        "samples",
        "truthy",
        r#"{ "state": "{{#if items.length}}has-items{{else}}empty{{/if}}" }"#,
    );
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/truthy"))
        .header("content-type", "application/json")
        .body(r#"{"items":[1]}"#)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["state"], "has-items");

    let resp = client
        .post(format!("{base}/samples/truthy"))
        .header("content-type", "application/json")
        .body(r#"{"items":[]}"#)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["state"], "empty");
}

// ---------------------------------------------------------------------------
// F-03 / D-007: form-encoded body must return structured 415.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f03_form_urlencoded_returns_unsupported_content_type() {
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("key=val&foo=bar")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 415);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "UnsupportedContentType");
    let msg = body["message"].as_str().unwrap();
    assert!(
        msg.contains("application/x-www-form-urlencoded"),
        "error message must name the type; got {msg}"
    );
}

#[tokio::test]
async fn f03_missing_content_type_still_works() {
    // JS DataMapper accepted requests with no explicit
    // Content-Type when the body was JSON; keep that.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .body(r#"{"ok":true}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn f03_vendor_json_content_type_is_accepted() {
    // `application/vnd.api+json` and friends should render, not 415.
    let tmp = TempDir::new().unwrap();
    write_dsl(tmp.path(), "samples", "echo", "{{{json this}}}");
    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/samples/echo"))
        .header("content-type", "application/vnd.api+json")
        .body(r#"{"a":1}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

// ---------------------------------------------------------------------------
// F-05 / R6.1: unknown top-level YAML fields must hard-fail at parse.
// ---------------------------------------------------------------------------

#[test]
fn f05_unknown_top_level_field_rejected() {
    let yaml = "port: 3000\ndsl_path: ./DSL\nbogus_field: 42\n";
    let result: Result<AppConfig, _> = serde_yaml_ng::from_str(yaml);
    assert!(
        result.is_err(),
        "expected parse failure on `bogus_field`, got Ok({:?})",
        result.ok()
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("bogus_field") || err.contains("unknown field"),
        "error must name the unknown field; got: {err}"
    );
}

#[test]
fn f05_unknown_nested_limit_field_rejected() {
    let yaml = "port: 3000\ndsl_path: ./DSL\nlimits:\n  max_request_bytes: 4096\n  wrongo: 1\n";
    let result: Result<AppConfig, _> = serde_yaml_ng::from_str(yaml);
    assert!(result.is_err(), "expected parse failure on nested `wrongo`");
}

// ---------------------------------------------------------------------------
// F-06 / D-002: request-limit default matches the documented 2 MiB
// (binary) and does NOT drift back to the JS "2mb" = 2 000 000 decimal.
// ---------------------------------------------------------------------------

#[test]
fn f06_request_limit_default_is_2_mib_binary() {
    let cfg = AppConfig::default();
    assert_eq!(
        cfg.limits.max_request_bytes,
        2 * 1024 * 1024,
        "default request-body cap must stay at 2 MiB binary; changing it \
         requires a DIVERGENCES.md entry (currently D-002)"
    );
}

// ---------------------------------------------------------------------------
// F-11 / D-016: the JS-compat single-line boot log line must be emitted
// alongside the structured tracing output. This is a build-target guard —
// we exercise the binary and grep stdout.
// ---------------------------------------------------------------------------

#[test]
fn f11_boot_emits_js_compatible_listening_line() {
    // The regression is at `println!("DataMapper listening on :{port}")`
    // in `src/main.rs`. We stand up the binary in a subprocess against
    // a temp workdir + a free port, wait for the line, and shut down.
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_datamapper");
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("DSL")).unwrap();
    let cfg = tmp.path().join("datamapper.yaml");
    std::fs::write(&cfg, "port: 0\ndsl_path: ./DSL\n").unwrap();

    let mut child = Command::new(bin)
        .current_dir(tmp.path())
        .arg("--config")
        .arg(&cfg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();

    let mut saw_line = false;
    let reader = BufReader::new(stdout);
    // Only read up to 100 lines to cap runtime on catastrophic failure.
    for (i, line) in reader.lines().enumerate() {
        let line = line.unwrap_or_default();
        if line.starts_with("DataMapper listening on :") {
            saw_line = true;
            break;
        }
        if i > 100 {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        saw_line,
        "expected `DataMapper listening on :<port>` on stdout — the JS DataMapper \
         boot-log line that operator log-grep monitors are keyed on (see D-016)"
    );
}
