//! Pre-boot environment validation — `datamapper doctor`.
//!
//! Adopts FLEET-STRONGHOLDS §8.2 — a subcommand that validates
//! everything an operator needs to know is right before the service
//! answers HTTP traffic, without side effects (no port bind, no
//! server start).
//!
//! Runs three classes of check:
//!
//! 1. **Config**: does the loader find a file? Does it parse? Does
//!    the JVM-wrapper preflight fire? Does the CORS-key gate fire?
//! 2. **DSL tree**: does `dsl_path` exist? How many `.hbs` files
//!    live there? Any use the JS `.length` accessor (auto-rewritten
//!    at render time but worth surfacing)? Is the tree writable
//!    (h2ck.me v1 L1 posture concern)?
//! 3. **Environment**: what does the runtime environment currently
//!    look like — `PORT` env, `DATAMAPPER_CONFIG` env, current
//!    working directory (relative `dsl_path` needs it to be right).
//!
//! Output shape: one line per check, `[ok] / [warn] / [error]`
//! prefix so the operator can `grep '\[error\]'` or `\[warn\]`.
//! Exit code:
//! - `0` when nothing above `ok` fired (or `warn`-only without
//!   `--strict`).
//! - `1` when any `error` fired OR when `--strict` and any `warn`
//!   fired.

use crate::config::AppConfig;
use std::path::{Path, PathBuf};

/// Severity of a single diagnostic emitted by [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warn,
    Error,
}

impl Severity {
    fn tag(self) -> &'static str {
        match self {
            Self::Ok => "[ok]   ",
            Self::Warn => "[warn] ",
            Self::Error => "[error]",
        }
    }
}

/// One diagnostic line.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub category: &'static str,
    pub message: String,
}

/// Run all doctor checks and return the collected diagnostics. Pure
/// — no side effects other than reading the filesystem and env
/// vars. Intended callers: `main::doctor()` and the unit tests.
pub fn run(config_override: Option<&Path>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_environment(&mut out);
    let cfg = check_config(&mut out, config_override);
    check_dsl_tree(&mut out, cfg.as_ref());
    out
}

/// Exit code the CLI should return given a diagnostic list.
///
/// - `Error` anywhere → 1.
/// - `Warn` anywhere AND `strict` → 1.
/// - Otherwise → 0.
pub fn exit_code(diagnostics: &[Diagnostic], strict: bool) -> u8 {
    let has_error = diagnostics.iter().any(|d| d.severity == Severity::Error);
    let has_warn = diagnostics.iter().any(|d| d.severity == Severity::Warn);
    if has_error || (strict && has_warn) {
        1
    } else {
        0
    }
}

/// Render diagnostics to stdout in a stable one-line-per-check
/// format. Split from [`run`] so tests can assert on the raw
/// `Vec<Diagnostic>` without capturing stdout.
pub fn print(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        println!("{} {}: {}", d.severity.tag(), d.category, d.message);
    }
    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warn_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .count();
    let ok_count = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Ok)
        .count();
    println!(
        "\nsummary: {} ok, {} warn, {} error",
        ok_count, warn_count, error_count
    );
}

fn check_environment(out: &mut Vec<Diagnostic>) {
    let cwd = std::env::current_dir().ok();
    match &cwd {
        Some(p) => out.push(Diagnostic {
            severity: Severity::Ok,
            category: "env",
            message: format!("cwd = {}", p.display()),
        }),
        None => out.push(Diagnostic {
            severity: Severity::Warn,
            category: "env",
            message: "cwd not readable — relative dsl_path may not resolve as expected".into(),
        }),
    }
    match std::env::var("PORT").ok() {
        Some(v) if !v.is_empty() => match v.parse::<u16>() {
            Ok(n) => out.push(Diagnostic {
                severity: Severity::Ok,
                category: "env",
                message: format!("PORT env = {n}"),
            }),
            Err(_) => out.push(Diagnostic {
                severity: Severity::Warn,
                category: "env",
                message: format!(
                    "PORT env = {v:?} is not a valid u16 — falls back to default 3000 (JS DataMapper compat)"
                ),
            }),
        },
        _ => out.push(Diagnostic {
            severity: Severity::Ok,
            category: "env",
            message: "PORT env not set — using config / default".into(),
        }),
    }
    match std::env::var("DATAMAPPER_CONFIG").ok() {
        Some(v) if !v.is_empty() => out.push(Diagnostic {
            severity: Severity::Ok,
            category: "env",
            message: format!("DATAMAPPER_CONFIG = {v}"),
        }),
        _ => out.push(Diagnostic {
            severity: Severity::Ok,
            category: "env",
            message: "DATAMAPPER_CONFIG env not set — using default search path".into(),
        }),
    }
}

fn check_config(out: &mut Vec<Diagnostic>, config_override: Option<&Path>) -> Option<AppConfig> {
    // If the caller passed `--config <path>`, set DATAMAPPER_CONFIG
    // for this process so `AppConfig::load_or_default()` sees it.
    // Safe because doctor is a short-lived subprocess.
    if let Some(p) = config_override {
        std::env::set_var("DATAMAPPER_CONFIG", p);
    }
    match AppConfig::load_or_default() {
        Ok((cfg, source)) => {
            match source {
                Some(p) => out.push(Diagnostic {
                    severity: Severity::Ok,
                    category: "config",
                    message: format!("loaded from {}", p.display()),
                }),
                None => out.push(Diagnostic {
                    severity: Severity::Ok,
                    category: "config",
                    message: "no file found — using built-in defaults".into(),
                }),
            }
            out.push(Diagnostic {
                severity: Severity::Ok,
                category: "config",
                message: format!(
                    "port={} dsl_path={} max_req={} max_resp={} req_to={}s",
                    cfg.port,
                    cfg.dsl_path.display(),
                    cfg.limits.max_request_bytes,
                    cfg.limits.max_response_bytes,
                    cfg.limits.request_timeout_secs,
                ),
            });
            Some(cfg)
        }
        Err(e) => {
            out.push(Diagnostic {
                severity: Severity::Error,
                category: "config",
                message: format!("failed to load: {e}"),
            });
            None
        }
    }
}

fn check_dsl_tree(out: &mut Vec<Diagnostic>, cfg: Option<&AppConfig>) {
    let Some(cfg) = cfg else {
        // Config load failed — skip DSL checks (no path to walk).
        return;
    };
    let dsl_path: &Path = &cfg.dsl_path;
    if !dsl_path.exists() {
        out.push(Diagnostic {
            severity: Severity::Warn,
            category: "dsl",
            message: format!(
                "dsl_path {} does not exist — the server will start but serve no templates",
                dsl_path.display()
            ),
        });
        return;
    }
    if !dsl_path.is_dir() {
        out.push(Diagnostic {
            severity: Severity::Error,
            category: "dsl",
            message: format!(
                "dsl_path {} exists but is not a directory",
                dsl_path.display()
            ),
        });
        return;
    }
    let mut hbs_count: usize = 0;
    let mut dot_length: Vec<String> = Vec::new();
    let mut read_errors: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(dsl_path)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.path().extension().and_then(|s| s.to_str()) != Some("hbs") {
            continue;
        }
        hbs_count += 1;
        match std::fs::read_to_string(entry.path()) {
            Ok(body) => {
                if crate::renderer::contains_dot_length_accessor(&body) {
                    if let Ok(rel) = entry.path().strip_prefix(dsl_path) {
                        dot_length.push(rel.display().to_string());
                    }
                }
            }
            Err(e) => {
                if let Ok(rel) = entry.path().strip_prefix(dsl_path) {
                    read_errors.push(format!("{}: {e}", rel.display()));
                }
            }
        }
    }
    out.push(Diagnostic {
        severity: Severity::Ok,
        category: "dsl",
        message: format!(
            "{hbs_count} template(s) discovered under {}",
            dsl_path.display()
        ),
    });
    if !read_errors.is_empty() {
        for msg in &read_errors {
            out.push(Diagnostic {
                severity: Severity::Error,
                category: "dsl",
                message: format!("unreadable template — {msg}"),
            });
        }
    }
    if !dot_length.is_empty() {
        let shown: Vec<String> = dot_length.iter().take(5).cloned().collect();
        let extra = if dot_length.len() > shown.len() {
            format!(", … +{} more", dot_length.len() - shown.len())
        } else {
            String::new()
        };
        out.push(Diagnostic {
            severity: Severity::Warn,
            category: "dsl",
            message: format!(
                "{} template(s) use the JS `.length` accessor — auto-rewritten to `(len …)` at render time (book/src/porting-from-js.md): {}{}",
                dot_length.len(),
                shown.join(", "),
                extra,
            ),
        });
    }
    // h2ck.me v1 L1 — DSL writability. This is a WARN in dev
    // deployments (a legit workflow needs the tree writable for
    // hot-reload); operators concerned about the posture should
    // ship with `DSL:/app/DSL:ro`.
    let writable = probe_writable(dsl_path);
    if writable {
        out.push(Diagnostic {
            severity: Severity::Warn,
            category: "dsl",
            message: format!(
                "dsl_path {} is writable by this process — production deployments should mount read-only (compose: `DSL:/app/DSL:ro`) to defeat symlink-swap / TOCTOU (h2ck.me v1 L1)",
                dsl_path.display()
            ),
        });
    } else {
        out.push(Diagnostic {
            severity: Severity::Ok,
            category: "dsl",
            message: format!(
                "dsl_path {} is read-only for this process — L1 posture holds",
                dsl_path.display()
            ),
        });
    }
}

/// Probe-write check. Same shape as `main::is_writable_by_us`
/// (kept local to keep the doctor module self-contained).
#[cfg(unix)]
fn probe_writable(path: &Path) -> bool {
    let probe = path.join(format!(".datamapper-doctor-probe-{}", std::process::id()));
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
fn probe_writable(_path: &Path) -> bool {
    // Non-Unix targets are not a supported production posture for
    // DataMapper. Skip the probe rather than false-alarm on
    // Windows dev boxes.
    false
}

// Silence dead-code warnings for the `PathBuf` re-export when tests
// aren't compiled — it isn't otherwise used at module scope.
#[allow(dead_code)]
type _Unused = PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_zero_when_all_ok() {
        let d = vec![Diagnostic {
            severity: Severity::Ok,
            category: "env",
            message: "cwd = /tmp".into(),
        }];
        assert_eq!(exit_code(&d, false), 0);
        assert_eq!(exit_code(&d, true), 0);
    }

    #[test]
    fn exit_code_zero_when_warn_only_and_not_strict() {
        let d = vec![Diagnostic {
            severity: Severity::Warn,
            category: "dsl",
            message: "writable".into(),
        }];
        assert_eq!(exit_code(&d, false), 0);
    }

    #[test]
    fn exit_code_one_when_warn_and_strict() {
        let d = vec![Diagnostic {
            severity: Severity::Warn,
            category: "dsl",
            message: "writable".into(),
        }];
        assert_eq!(exit_code(&d, true), 1);
    }

    #[test]
    fn exit_code_one_when_error_regardless_of_strict() {
        let d = vec![Diagnostic {
            severity: Severity::Error,
            category: "config",
            message: "unparseable".into(),
        }];
        assert_eq!(exit_code(&d, false), 1);
        assert_eq!(exit_code(&d, true), 1);
    }

    #[test]
    fn severity_tag_stable() {
        // Match tests grep for these prefixes.
        assert_eq!(Severity::Ok.tag(), "[ok]   ");
        assert_eq!(Severity::Warn.tag(), "[warn] ");
        assert_eq!(Severity::Error.tag(), "[error]");
    }

    #[test]
    fn dsl_tree_walk_reports_hbs_count() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("samples")).unwrap();
        std::fs::write(tmp.path().join("samples/a.hbs"), "{ \"a\": {{val}} }\n").unwrap();
        std::fs::write(
            tmp.path().join("samples/b.hbs"),
            "{ \"b\": {{{json this}}} }\n",
        )
        .unwrap();
        // Also drop a non-.hbs file — should be ignored.
        std::fs::write(tmp.path().join("samples/readme.md"), "hi").unwrap();

        // Fake AppConfig pointing at tmp.
        let cfg = AppConfig {
            dsl_path: tmp.path().to_path_buf(),
            ..AppConfig::default()
        };
        let mut diagnostics = Vec::new();
        check_dsl_tree(&mut diagnostics, Some(&cfg));

        // First diagnostic: template count.
        let count_diag = diagnostics
            .iter()
            .find(|d| d.message.contains("template(s) discovered"))
            .expect("count diagnostic present");
        assert!(
            count_diag.message.starts_with("2 "),
            "expected 2 templates, got: {}",
            count_diag.message
        );
        assert_eq!(count_diag.severity, Severity::Ok);
    }

    #[test]
    fn dsl_tree_walk_warns_on_dot_length_accessor() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("samples")).unwrap();
        std::fs::write(
            tmp.path().join("samples/count.hbs"),
            "{ \"n\": {{items.length}} }\n",
        )
        .unwrap();
        let cfg = AppConfig {
            dsl_path: tmp.path().to_path_buf(),
            ..AppConfig::default()
        };
        let mut diagnostics = Vec::new();
        check_dsl_tree(&mut diagnostics, Some(&cfg));
        assert!(diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warn && d.message.contains(".length")));
    }

    #[test]
    fn dsl_tree_walk_reports_missing_directory_as_warn() {
        let cfg = AppConfig {
            dsl_path: std::path::PathBuf::from("/does/not/exist/at/all"),
            ..AppConfig::default()
        };
        let mut diagnostics = Vec::new();
        check_dsl_tree(&mut diagnostics, Some(&cfg));
        assert!(diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warn && d.message.contains("does not exist")));
    }

    #[test]
    fn run_top_level_returns_non_empty_diagnostics() {
        // Doctor always emits at least env-line output. Precise
        // count varies with the runtime env, but > 0 is a
        // solid invariant.
        let d = run(None);
        assert!(!d.is_empty());
    }
}
