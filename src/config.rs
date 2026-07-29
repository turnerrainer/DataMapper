//! DataMapper-on-Rust configuration.
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
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_dsl_path")]
    pub dsl_path: PathBuf,

    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub fn load_or_default() -> Result<(Self, Option<PathBuf>), DataMapperError> {
        for path in config_search_paths() {
            if path.exists() {
                let body = std::fs::read_to_string(&path).map_err(|e| {
                    DataMapperError::Internal(format!("reading config {}: {}", path.display(), e))
                })?;
                let cfg: AppConfig = serde_yaml_ng::from_str(&body).map_err(|e| {
                    DataMapperError::Internal(format!("parsing config {}: {}", path.display(), e))
                })?;
                return Ok((cfg, Some(path)));
            }
        }
        Ok((Self::default(), None))
    }
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
}
