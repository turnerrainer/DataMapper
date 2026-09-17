//! DataMapper entry point.
//!
//! Assembles: config → renderer → axum router → server. Binds on
//! `0.0.0.0:<config.port>`.
//!
//! **Subcommands** (FLEET §8.2):
//! - default / `serve` — run the HTTP server (behaviour identical
//!   to pre-clap boot).
//! - `doctor [--strict]` — validate config + DSL tree + env
//!   without starting the server. Prints one line per check with a
//!   `[ok]/[warn]/[error]` prefix; exits `1` on any error (or on
//!   any warn when `--strict`).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use datamapper::{
    config::AppConfig,
    doctor, env_safety,
    renderer::Renderer,
    router::{self, AppState},
    shutdown,
};

#[derive(Parser)]
#[command(name = "datamapper", version, about, long_about = None)]
struct Cli {
    /// Explicit config file path — overrides the
    /// `DATAMAPPER_CONFIG` env and the default search order.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (default).
    Serve,
    /// Validate config + DSL tree + environment without starting
    /// the server. Prints one line per check; exits non-zero on
    /// error.
    Doctor {
        /// Exit non-zero on any WARN, not just ERROR.
        #[arg(long)]
        strict: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Set DATAMAPPER_CONFIG from --config once, so both `serve`
    // and `doctor` see the same config-search behaviour.
    if let Some(path) = &cli.config {
        std::env::set_var("DATAMAPPER_CONFIG", path);
    }
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Error: failed to start tokio runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match rt.block_on(serve()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {e:?}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Doctor { strict } => run_doctor(cli.config.as_deref(), strict),
    }
}

fn run_doctor(config_override: Option<&std::path::Path>, strict: bool) -> ExitCode {
    // Doctor deliberately does NOT initialise tracing — its output
    // goes to stdout in a stable one-line-per-check format that
    // operators can grep. Tracing to stderr would mix in noise.
    let diagnostics = doctor::run(config_override);
    doctor::print(&diagnostics);
    match doctor::exit_code(&diagnostics, strict) {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

async fn serve() -> anyhow::Result<()> {
    // Audit LOG-v1 FN-LOG-1: emit ANSI colour codes only when stderr is
    // a TTY. Under Docker / systemd, ship plain-text logs for SIEM.
    use std::io::IsTerminal;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(std::io::stderr().is_terminal())
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
    // h2ck.me v1 N1 — WARN per template that uses a non-json
    // triple-brace (`{{{X}}}`) mustache expression. The runtime
    // backstop in `router::respond` forces `text/plain` on those
    // templates' fallback responses, but boot visibility lets
    // operators find and audit the affected DSLs.
    warn_on_unsafe_raw_output_templates(&cfg.dsl_path);

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
        max_body_array_length: cfg.limits.max_body_array_length,
        request_timeout_secs: cfg.limits.request_timeout_secs,
    };

    let app = router::build(state);
    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("listening on {}", addr);
    // Back-compat with the JS DataMapper boot line so log-grep
    // patterns keyed on `DataMapper listening on :<port>` keep
    // working. See book/src/porting-from-js.md.
    println!("DataMapper listening on :{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    // h2ck.me v1 NEXT-TASKS.md §T-18 — orchestrator-issued
    // SIGTERM / SIGINT / SIGHUP triggers a graceful drain: the
    // acceptor stops taking new connections while in-flight
    // requests finish. Combined with the `TimeoutLayer` above,
    // an unresponsive template can only delay shutdown by
    // `request_timeout_secs`.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown::shutdown_signal())
        .await?;
    tracing::info!("shutdown complete");
    Ok(())
}

/// Emit a per-template WARN for every `.hbs` under `dsl_root` that
/// contains a `{{{X}}}` triple-brace mustache expression whose inner
/// form is NOT the built-in `json` helper. See h2ck.me v1 N1 in
/// `SECURITY.md` for the composite-XSS lane this catches.
///
/// The runtime backstop in `router::respond` already deterministic-
/// ally forces `text/plain` on affected templates' fallback
/// responses — this walk exists so an operator (a) sees which files
/// need auditing at boot time and (b) can excise the raw output if
/// the response was legitimately expected to be HTML.
fn warn_on_unsafe_raw_output_templates(dsl_root: &std::path::Path) {
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
        if datamapper::renderer::contains_unsafe_raw_output(&body) {
            if let Ok(rel) = entry.path().strip_prefix(dsl_root) {
                affected.push(rel.display().to_string());
            }
        }
    }
    if !affected.is_empty() {
        let shown: Vec<String> = affected.iter().take(10).cloned().collect();
        tracing::warn!(
            "{} template(s) under {} use `{{{{{{X}}}}}}` (non-json triple-brace / raw output) — the runtime forces text/plain on their fallback responses (h2ck.me v1 N1). Audit and prefer `{{{{X}}}}` (double-brace, HTML-escaped) unless the response is JSON-shaped: {}{}",
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

/// Return true if the DSL root — or any subdirectory beneath it,
/// up to depth 3 — is writable by the current process UID.
///
/// Uses a probe-file write (`OpenOptions::create_new(true).write(true)`)
/// rather than a mode-bit check so that Docker `:ro` mount overrides
/// are honoured — a directory with `0755` mode still returns `false`
/// when the underlying mount is read-only, which is the case we
/// actually care about.
///
/// Consumed by the FLEET §11.2 posture check (drives boot refusal
/// in non-dev, WARN-only in dev).
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
