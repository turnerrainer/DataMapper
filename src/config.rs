//! DataMapper configuration.
//!
//! Wire shape (`datamapper.yaml`):
//!
//! ```yaml
//! port: 3000
//! dsl_path: ./DSL
//! limits:
//!   max_request_bytes: 2097152
//!   max_response_bytes: 16777216
//!   request_timeout_secs: 30
//! ```
//!
//! Search order for the file:
//! 1. `--config <path>` CLI flag
//! 2. `DATAMAPPER_CONFIG` env var
//! 3. `./datamapper.yaml` or `./datamapper.yml`
//! 4. Built-in defaults if nothing is found

use crate::error::DataMapperError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_dsl_path")]
    pub dsl_path: PathBuf,

    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "default_max_request_bytes")]
    pub max_request_bytes: usize,

    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,

    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: default_max_request_bytes(),
            max_response_bytes: default_max_response_bytes(),
            request_timeout_secs: default_request_timeout_secs(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            dsl_path: default_dsl_path(),
            limits: Limits::default(),
        }
    }
}

fn default_port() -> u16 {
    3000
}
fn default_dsl_path() -> PathBuf {
    PathBuf::from("./DSL")
}
fn default_max_request_bytes() -> usize {
    2 * 1024 * 1024 // 2 MiB
}
fn default_max_response_bytes() -> usize {
    16 * 1024 * 1024 // 16 MiB
}
fn default_request_timeout_secs() -> u64 {
    30
}

impl AppConfig {
    /// Resolve, load, and return the operator's `AppConfig`. Falls
    /// back to defaults if no file is found on any conventional path.
    /// Returns `(config, source_path_or_none)` — caller logs which
    /// took effect.
    ///
    /// Post-load, `PORT` env var (JS DataMapper compat, see
    /// `book/src/porting-from-js.md`) overrides the loaded/default
    /// `port` field when the loaded config did not explicitly set it.
    /// The explicit-set signal is: config file present AND contains
    /// a `port:` key.
    pub fn load_or_default() -> Result<(Self, Option<PathBuf>), DataMapperError> {
        for path in config_search_paths() {
            if path.exists() {
                let body = std::fs::read_to_string(&path).map_err(|e| {
                    DataMapperError::Internal(format!("reading config {}: {}", path.display(), e))
                })?;
                // h2ck.me v1 I1 — DataMapper does NOT terminate CORS.
                // Historically an operator could set `cors_origin: …`
                // in their yaml and the server would silently drop it
                // behind a boot WARN, which is the worst-of-both:
                // browsers still block, operators think they've
                // configured CORS, and the workaround they reach for
                // is often unsafe (adding `crossorigin=anonymous`,
                // hand-rolled `Access-Control-Allow-Origin: *`, etc.).
                // Refuse to start instead so misconfiguration surfaces
                // at boot with a clear pointer to the correct layer.
                if let Some(line_no) = find_cors_origin_key(&body) {
                    return Err(DataMapperError::Internal(format!(
                        "config {} line {}: `cors_origin` is not implemented in this release — \
                         DataMapper does not terminate CORS. Configure CORS at your reverse proxy \
                         (nginx `add_header Access-Control-Allow-Origin`, Traefik middleware, etc.). \
                         Remove the `cors_origin` key from the config to boot.",
                        path.display(),
                        line_no
                    )));
                }
                let cfg: AppConfig = serde_yaml_ng::from_str(&body).map_err(|e| {
                    DataMapperError::Internal(format!("parsing config {}: {}", path.display(), e))
                })?;
                let file_sets_port = body.lines().any(|l| {
                    l.trim_start().starts_with("port:") && !l.trim_start().starts_with("#")
                });
                let cfg = if file_sets_port {
                    cfg
                } else {
                    apply_port_env(cfg)
                };
                return Ok((cfg, Some(path)));
            }
        }
        Ok((apply_port_env(Self::default()), None))
    }
}

fn apply_port_env(mut cfg: AppConfig) -> AppConfig {
    if let Ok(p) = std::env::var("PORT") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            match trimmed.parse::<u16>() {
                Ok(port) => cfg.port = port,
                Err(_) => tracing::warn!(
                    "PORT env var '{}' is not a valid u16 port number; ignoring (JS DataMapper compat)",
                    trimmed
                ),
            }
        }
    }
    cfg
}

/// Return the 1-based line number of a top-level `cors_origin:` key
/// in the YAML text, or `None` if not present. Ignores commented
/// lines and any occurrence inside a nested mapping (indented lines).
///
/// See M2 rationale on `AppConfig::load_or_default` — DataMapper
/// intentionally refuses to boot when this key is set so silent
/// drop behind a boot WARN cannot mislead operators.
fn find_cors_origin_key(yaml: &str) -> Option<usize> {
    for (idx, raw) in yaml.lines().enumerate() {
        let no_indent = raw.trim_start();
        if no_indent.starts_with('#') {
            continue;
        }
        // Only match top-level keys (no leading whitespace) so a
        // future `some_helper:\n  cors_origin: foo` inside an unrelated
        // subtree doesn't false-positive.
        if raw.starts_with("cors_origin:") || raw.starts_with("cors_origin ") {
            return Some(idx + 1);
        }
    }
    None
}

fn config_search_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            out.push(PathBuf::from(&args[i + 1]));
            break;
        }
        if let Some(rest) = args[i].strip_prefix("--config=") {
            out.push(PathBuf::from(rest));
            break;
        }
        i += 1;
    }
    if let Ok(p) = std::env::var("DATAMAPPER_CONFIG") {
        if !p.is_empty() {
            out.push(PathBuf::from(p));
        }
    }
    out.push(PathBuf::from("./datamapper.yaml"));
    out.push(PathBuf::from("./datamapper.yml"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.port, 3000);
        assert_eq!(cfg.dsl_path, PathBuf::from("./DSL"));
        assert_eq!(cfg.limits.max_request_bytes, 2 * 1024 * 1024);
        assert_eq!(cfg.limits.max_response_bytes, 16 * 1024 * 1024);
        assert_eq!(cfg.limits.request_timeout_secs, 30);
    }

    #[test]
    fn parses_full_yaml() {
        let yaml = r#"
port: 8080
dsl_path: /custom/DSL
limits:
  max_request_bytes: 4096
  max_response_bytes: 8192
  request_timeout_secs: 5
"#;
        let cfg: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.dsl_path, PathBuf::from("/custom/DSL"));
        assert_eq!(cfg.limits.max_request_bytes, 4096);
        assert_eq!(cfg.limits.max_response_bytes, 8192);
        assert_eq!(cfg.limits.request_timeout_secs, 5);
    }

    #[test]
    fn partial_yaml_uses_defaults_for_missing_fields() {
        let yaml = "port: 9999\n";
        let cfg: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.dsl_path, PathBuf::from("./DSL"));
        assert_eq!(cfg.limits.max_request_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let yaml = "port: not-a-number\n";
        let result: Result<AppConfig, _> = serde_yaml_ng::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn cors_origin_key_is_detected_at_top_level() {
        assert_eq!(
            find_cors_origin_key("port: 3000\ncors_origin: \"*\"\n"),
            Some(2)
        );
        assert_eq!(find_cors_origin_key("cors_origin: foo\n"), Some(1));
    }

    #[test]
    fn cors_origin_key_ignores_indented_and_commented_occurrences() {
        assert_eq!(find_cors_origin_key("# cors_origin: foo\n"), None);
        assert_eq!(
            find_cors_origin_key("something:\n  cors_origin: nested\n"),
            None
        );
        assert_eq!(find_cors_origin_key("port: 3000\n"), None);
    }
}
