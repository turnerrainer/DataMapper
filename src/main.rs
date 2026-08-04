//! DataMapper entry point.
//!
//! Assembles: config → renderer → axum router → server. Binds on
//! `0.0.0.0:<config.port>`.

use std::sync::Arc;

use datamapper::{
    config::AppConfig,
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

    // REFACTO-REQUIREMENTS §6.2 — boot-time diagnostic pass.
    // R2.1 / D-003, D-004: warn if the operator has parked
    // Handlebars templates under a legacy `views/` directory that
    // JS DataMapper used to also serve from. Rust only searches
    // `dsl_path`, so those files would 404 silently otherwise.
    warn_on_legacy_views_dir();

    // §6.2 continued: scan the DSL tree for `.length` accessors so
    // the operator has a single boot-log signal for "which files
    // still lean on the JS compat rewriter" (D-010). Aggregated so
    // the log doesn't flood on large trees.
    warn_on_ported_js_dsl_syntax(&cfg.dsl_path);

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
    // working. See REFACTO-AUDIT-S2.md F-11 / D-016.
    println!("DataMapper listening on :{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
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
            "found {} .hbs file(s) under ./views/ — Rust DataMapper only serves templates from dsl_path (see DIVERGENCES.md D-003)",
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
            "{} template(s) under {} use the JS `.length` accessor and are being auto-rewritten via the compat helper (see DIVERGENCES.md D-010, MIGRATION.md §.length): {}{}",
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
