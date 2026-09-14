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
    doctor,
    renderer::Renderer,
    router::{self, AppState},
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
    // h2ck.me v1 L1 — DSL root MUST be read-only in production.
    // A writable mount lets a filesystem-writer replace a `.hbs`
    // with a symlink to any file the process can read (classic
    // TOCTOU / symlink swap between our loader's stat and the
    // subsequent `read_to_string`). The shipped compose file
    // already mounts `DSL:/app/DSL:ro`; this WARN catches operators
    // who deviated. Doesn't refuse-to-start — dev loops legitimately
    // want a writable tree — but names the path so the deviation
    // shows up in the boot log.
    warn_on_writable_dsl_root(&cfg.dsl_path);

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

/// Emit a WARN if the DSL root — or any subdirectory beneath it —
/// is writable by the current process UID. See L1 in `SECURITY.md`
/// for the deployment posture this guards against.
fn warn_on_writable_dsl_root(dsl_root: &std::path::Path) {
    if !dsl_root.exists() {
        return;
    }
    let mut writable_paths: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(dsl_root)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        if is_writable_by_us(entry.path()) {
            writable_paths.push(entry.path().display().to_string());
        }
        if writable_paths.len() >= 5 {
            break;
        }
    }
    if !writable_paths.is_empty() {
        tracing::warn!(
            "DSL root is writable by the DataMapper process ({}) — production deployments MUST mount the DSL tree read-only \
             (compose: `DSL:/app/DSL:ro`) to defeat symlink-swap and TOCTOU attacks on template files. See SECURITY.md.",
            writable_paths.join(", "),
        );
    }
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
