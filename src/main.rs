//! DataMapper entry point.
//!
//! Assembles: config → renderer → axum router → server. Binds on
//! `0.0.0.0:<config.port>`.

use std::sync::Arc;

use datamapper::{
    config::AppConfig,
    env_safety,
    renderer::Renderer,
    router::{self, AppState},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let version = env!("CARGO_PKG_VERSION");
    tracing::info!("datamapper v{} starting", version);

    let (cfg, cfg_source) = AppConfig::load_or_default()?;
    match cfg_source {
        Some(p) => tracing::info!("loaded config from {}", p.display()),
        None => tracing::info!("using built-in defaults (no datamapper.yaml found)"),
    }
    tracing::info!(
        "dsl_path={} port={} max_request_bytes={} max_response_bytes={}",
        cfg.dsl_path.display(),
        cfg.port,
        cfg.limits.max_request_bytes,
        cfg.limits.max_response_bytes,
    );

    // Boot-time diagnostics for operators porting from JS DataMapper.
    // Warn on legacy `./views/` .hbs files (JS served that root;
    // Rust does not).
    warn_on_legacy_views_dir();
    // Aggregate INFO listing DSL files still using the JS `.length`
    // accessor so operators know which files the compat rewriter is
    // fixing up under the hood.
    warn_on_ported_js_dsl_syntax(&cfg.dsl_path);
    // FLEET-STRONGHOLDS §11 — env-aware safety gates. In non-dev
    // environments (APP_ENV / ENVIRONMENT / DEPLOY_ENV) upgrade
    // documented-unsafe posture WARNs to hard REFUSALS. Unknown
    // env strings fail-safe to Production.
    let env = env_safety::Environment::from_env();
    tracing::info!(target: "env_safety", "detected environment: {env:?}");

    // §11.2 — posture checks. DataMapper has one posture flag
    // today: DSL root writability (h2ck.me v1 L1). Previously a
    // WARN in every env; now REFUSES in non-dev.
    let writable = is_dsl_root_writable(&cfg.dsl_path);
    let posture_checks = vec![env_safety::PostureCheck {
        name: "dsl.root_writable",
        is_safe: !writable,
        description: "DSL root is writable by the DataMapper process — a filesystem writer can \
             swap a .hbs for a symlink to any process-readable file (TOCTOU).",
        fix: "mount the DSL tree read-only (compose: `DSL:/app/DSL:ro`) OR set APP_ENV=dev",
    }];
    if let Err(msg) = env_safety::enforce_posture(env, &posture_checks) {
        return Err(anyhow::anyhow!(msg));
    }

    // §11.3 — dev-fixture DSL gate. Refuses to load any .hbs whose
    // path matches a documented dev/mock/test pattern
    // (`dev-login`, `mock-`, `-mock`, `/test/`, `example-`,
    // `-example`, `/dev/`, `/mocks/`) in non-dev environments. In
    // dev, matches produce WARN and boot continues.
    let dev_fixture_action = env_safety::DevFixtureAction::from_env(env);
    if let Err(msg) = env_safety::enforce_dev_fixture_scan(&cfg.dsl_path, dev_fixture_action) {
        return Err(anyhow::anyhow!(msg));
    }

    let state = AppState {
        renderer: Arc::new(Renderer::new(cfg.dsl_path.clone())),
        max_request_bytes: cfg.limits.max_request_bytes,
        max_response_bytes: cfg.limits.max_response_bytes,
    };

    let app = router::build(state);
    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("listening on {}", addr);
    // Back-compat with the JS DataMapper boot line so log-grep
    // patterns keyed on `DataMapper listening on :<port>` keep
    // working. See book/src/porting-from-js.md.
    println!("DataMapper listening on :{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Return true if the DSL root — or any subdirectory beneath it,
/// up to depth 3 — is writable by the current process UID.
///
/// Uses a probe-file write (`OpenOptions::create_new(true).write(true)`)
/// rather than a mode-bit check so that Docker `:ro` mount overrides
/// are honoured — a directory with `0755` mode still returns `false`
/// when the underlying mount is read-only, which is the case we
/// actually care about.
///
/// Consumed by both the FLEET §11.2 posture check (drives boot
/// refusal in non-dev) and, indirectly, by the ops-facing WARN log
/// line the posture check emits in dev.
fn is_dsl_root_writable(dsl_root: &std::path::Path) -> bool {
    if !dsl_root.exists() {
        return false;
    }
    for entry in walkdir::WalkDir::new(dsl_root)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if is_writable_by_us(entry.path()) {
            tracing::debug!(path = %entry.path().display(), "writable DSL directory");
            return true;
        }
    }
    false
}

/// Best-effort check: can this process write into `path`?
/// Uses a probe file rather than reading mode bits so mount-level
/// read-only overrides (Docker `:ro`) are honoured — a directory
/// with `0755` mode bits still returns `false` when the underlying
/// mount is read-only, which is the case we actually care about.
#[cfg(unix)]
fn is_writable_by_us(path: &std::path::Path) -> bool {
    // Use a unique per-boot probe filename so two racing DataMapper
    // instances (which they should never be, but still) don't clobber
    // each other's probe.
    let probe = path.join(format!(".datamapper-writable-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_writable_by_us(_path: &std::path::Path) -> bool {
    // Non-Unix targets are not a supported production posture for
    // DataMapper (container-shipped, Linux only). Skip the probe
    // rather than issuing a false alarm on Windows dev boxes.
    false
}

fn warn_on_legacy_views_dir() {
    let views = std::path::Path::new("./views");
    if !views.is_dir() {
        return;
    }
    let hbs_count = walkdir::WalkDir::new(views)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("hbs"))
        .count();
    if hbs_count > 0 {
        tracing::warn!(
            "found {} .hbs file(s) under ./views/ — Rust DataMapper only serves templates from dsl_path (see book/src/porting-from-js.md)",
            hbs_count
        );
    }
}

fn warn_on_ported_js_dsl_syntax(dsl_root: &std::path::Path) {
    if !dsl_root.is_dir() {
        return;
    }
    let mut affected: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(dsl_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.path().extension().and_then(|s| s.to_str()) != Some("hbs") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if datamapper::renderer::contains_dot_length_accessor(&body) {
            if let Ok(rel) = entry.path().strip_prefix(dsl_root) {
                affected.push(rel.display().to_string());
            }
        }
    }
    if !affected.is_empty() {
        // Cap the file list at 10 in the log so an unmigrated tree
        // does not flood the boot log.
        let shown: Vec<String> = affected.iter().take(10).cloned().collect();
        tracing::info!(
            "{} template(s) under {} use the JS `.length` accessor and are being auto-rewritten via the compat helper (see book/src/porting-from-js.md): {}{}",
            affected.len(),
            dsl_root.display(),
            shown.join(", "),
            if affected.len() > shown.len() {
                format!(", … +{} more", affected.len() - shown.len())
            } else {
                String::new()
            },
        );
    }
}
