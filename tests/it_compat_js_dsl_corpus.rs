//! REFACTO-REQUIREMENTS §7.3 — syntactic corpus of JS source-of-truth
//! DSLs must run through the Rust parser without unexpected failures.
//!
//! Every `.hbs` file under `compat/js-DSL/` is a verbatim copy of a
//! `Buerostack/DataMapper/DSL/*.hbs` file. This test loads each,
//! POSTs the exact JSON body that the file's inline curl comment
//! recommends, and asserts the render succeeds. A silent regression
//! shows up as a failing row; a real divergence (e.g. a template
//! using a syntax the Rust engine cannot resolve) must be paired
//! with a `book/src/porting-from-js.md` entry before this test can be marked
//! `#[ignore]` on a per-file basis.

use datamapper::{
    renderer::Renderer,
    router::{self, AppState},
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

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

#[tokio::test]
async fn js_source_of_truth_corpus_renders_end_to_end() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("compat/js-DSL");
    assert!(
        corpus.exists(),
        "compat corpus missing at {}; refacto batch was supposed to \
         copy Buerostack/DataMapper/DSL there (see compat/README.md)",
        corpus.display()
    );

    let tmp = TempDir::new().unwrap();
    copy_dir(&corpus, tmp.path());

    let base = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    // (path, body) pairs — extracted from the curl comment inside
    // each JS-source DSL. If the JS source ships a new sample,
    // append its route + body here; a missing row is a coverage
    // gap, not a green test.
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
            // JS-shipped template uses `{{products.length}}`.
            // Rust must render it (D-010 rewriter).
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

    // Sanity check: every .hbs file under the corpus MUST appear in
    // `cases`. A new sample without a row here is a silent coverage
    // gap and R3.4 forbids silent gaps.
    let mut expected: Vec<String> = walkdir::WalkDir::new(&corpus)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("hbs"))
        .map(|e| {
            let rel = e.path().strip_prefix(&corpus).unwrap().to_path_buf();
            rel.with_extension("").to_string_lossy().into_owned()
        })
        .collect();
    expected.sort();
    let mut covered: Vec<String> = cases.iter().map(|(p, _)| p.to_string()).collect();
    covered.sort();
    assert_eq!(
        expected, covered,
        "compat corpus and test-case list drifted — \
         either the JS source shipped a new sample without updating \
         this file, or a case here doesn't exist in the corpus"
    );

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
        assert_eq!(
            status, 200,
            "JS source-of-truth DSL {path} did not render 200 on Rust — \
             either fix the Rust engine or add a book/src/porting-from-js.md entry \
             pinning down the intentional gap. Body: {text}"
        );
        let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!("JS source-of-truth DSL {path} did not return JSON: {e}\nBody: {text}")
        });
        assert!(parsed.is_object() || parsed.is_array(), "{path} → {parsed}");
    }
}
