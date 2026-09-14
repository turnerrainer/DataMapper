//! Environment-aware safety gates.
//!
//! Adopts FLEET-STRONGHOLDS §11 — the cross-service pattern for
//! preventing "dev defaults shipped to non-dev environments" (the
//! single most common "found in production" pentest finding across
//! the Buerostack fleet + eFTI Gate EE audits).
//!
//! The rule: in any environment above `Dev`, boot MUST refuse when
//! - a documented-unsafe security posture flag is set (§11.2); or
//! - the DSL tree contains a dev-fixture template (§11.3).
//!
//! In `Dev`, the same conditions emit `tracing::warn!` and boot
//! continues — DataMapper's zero-config local dev experience stays
//! intact.
//!
//! Environment is read from `APP_ENV`, `ENVIRONMENT`, or `DEPLOY_ENV`
//! (first match wins). Missing or unknown values fail-safe to
//! `Production` so that a forgotten env var can't downgrade the
//! posture silently.
//!
//! **DataMapper-specific notes.** DataMapper has no admin token, no
//! DB credentials, and no runtime secrets — §11.1 (weak-credentials
//! gate) is a no-op here. Only §11.2 (posture) and §11.3 (dev
//! fixtures) are wired.

use std::sync::OnceLock;

/// Deployment environment class. `Dev` permits weak defaults;
/// every other variant treats them as fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Dev,
    Test,
    Staging,
    Production,
}

impl Environment {
    /// Cache the result of the first call so the value doesn't
    /// change under an operator's feet mid-boot. Tests use
    /// [`from_env_str`] for injectability.
    pub fn from_env() -> Self {
        static CACHED: OnceLock<Environment> = OnceLock::new();
        *CACHED.get_or_init(|| {
            let raw = std::env::var("APP_ENV")
                .or_else(|_| std::env::var("ENVIRONMENT"))
                .or_else(|_| std::env::var("DEPLOY_ENV"))
                .unwrap_or_else(|_| "dev".to_string());
            Self::from_env_str(&raw)
        })
    }

    /// Pure classifier — testable, no `OnceLock`. Unknown values
    /// fail-safe to `Production`.
    pub fn from_env_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" | "development" | "local" => Self::Dev,
            "test" | "testing" | "ci" => Self::Test,
            "stage" | "staging" | "preprod" => Self::Staging,
            "prod" | "production" | "live" => Self::Production,
            _ => {
                // Fail-safe: unknown env → treat as Production so
                // that a forgotten env var (or a typo like
                // `APP_ENV=prroduction`) doesn't silently downgrade
                // the security posture.
                eprintln!(
                    "[env_safety] WARN: unknown environment `{raw}` — treating as Production"
                );
                Self::Production
            }
        }
    }

    /// True for anything above `Dev`. In these envs, unsafe posture
    /// items and dev fixtures refuse to boot rather than emit WARN.
    pub fn requires_prod_creds(self) -> bool {
        !matches!(self, Self::Dev)
    }
}

/// One security-posture flag to validate at boot.
///
/// Each `PostureCheck` describes a documented-unsafe flag that is
/// permitted in `Dev` (with a WARN) but refused in any other env.
pub struct PostureCheck {
    pub name: &'static str,
    /// True when the current config / deployment is safe.
    pub is_safe: bool,
    /// Human explanation of the risk when `!is_safe`.
    pub description: &'static str,
    /// Concrete remediation hint.
    pub fix: &'static str,
}

/// Boot-time gate for security-posture flags. Returns Err in non-dev
/// when any check fails. In dev, all failures are emitted as WARN
/// and the function returns Ok.
pub fn enforce_posture(env: Environment, checks: &[PostureCheck]) -> Result<(), String> {
    let unsafe_items: Vec<&PostureCheck> = checks.iter().filter(|c| !c.is_safe).collect();
    if unsafe_items.is_empty() {
        return Ok(());
    }
    if env.requires_prod_creds() {
        return Err(format!(
            "REFUSING TO START in {env:?}: {} unsafe posture item(s):\n{}",
            unsafe_items.len(),
            unsafe_items
                .iter()
                .map(|c| format!("  - {}: {}\n      fix: {}", c.name, c.description, c.fix))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    for c in unsafe_items {
        tracing::warn!(
            target: "env_safety",
            "unsafe posture (dev env, permitted): {} — {} — fix: {}",
            c.name, c.description, c.fix,
        );
    }
    Ok(())
}

/// Substring patterns that identify a DSL file as a dev/mock/test
/// fixture. Case-insensitive, matched against the full path
/// (POSIX-slash normalised). Cross-fleet patterns from
/// FLEET-STRONGHOLDS §11.3.
///
/// Kept narrow so the shipped `DSL/samples/` tree stays loadable —
/// none of its files (`ping.hbs`, `echo.hbs`, `flatten_address.hbs`,
/// etc.) trip any pattern in this list.
pub const DEV_ONLY_DSL_PATTERNS: &[&str] = &[
    "dev-login",
    "mock-",
    "-mock",
    "/test/",
    "example-",
    "-example",
    "/dev/",
    "/mocks/",
];

/// Return the pattern that matches `path_str` (POSIX-slash
/// normalised, lowercased) if any. `None` when the path is not a
/// dev fixture.
pub fn dev_fixture_pattern(path_str: &str) -> Option<&'static str> {
    // Normalise Windows separators so tests written on Linux CI
    // stay portable and a Windows dev doesn't accidentally bypass
    // the check with backslashes.
    let normalised = path_str.replace('\\', "/").to_ascii_lowercase();
    DEV_ONLY_DSL_PATTERNS
        .iter()
        .copied()
        .find(|pat| normalised.contains(pat))
}

/// Boot-time action to take when a DSL file matches a dev-fixture
/// pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevFixtureAction {
    /// No gate — permissive default for unit tests.
    Ignore,
    /// Emit a WARN and continue loading. Used in `Dev`.
    Warn,
    /// Return a fatal load error. Used in every non-dev env.
    Refuse,
}

impl DevFixtureAction {
    /// Compute the correct action from the detected environment.
    /// Dev → Warn (continue), everything else → Refuse (abort).
    pub fn from_env(env: Environment) -> Self {
        if env.requires_prod_creds() {
            Self::Refuse
        } else {
            Self::Warn
        }
    }
}

/// Walk `dsl_root` and enforce the §11.3 dev-fixture gate against
/// the configured [`DevFixtureAction`].
///
/// - `DevFixtureAction::Ignore` → no-op, returns Ok immediately.
/// - `DevFixtureAction::Warn` → walks the tree, emits WARN per
///   match, always returns Ok.
/// - `DevFixtureAction::Refuse` → walks the tree; on FIRST match,
///   returns Err with a message naming the file and matched
///   pattern. Loading is not attempted.
pub fn enforce_dev_fixture_scan(
    dsl_root: &std::path::Path,
    action: DevFixtureAction,
) -> Result<(), String> {
    if matches!(action, DevFixtureAction::Ignore) {
        return Ok(());
    }
    if !dsl_root.is_dir() {
        return Ok(());
    }
    let mut matched: Vec<(std::path::PathBuf, &'static str)> = Vec::new();
    for entry in walkdir::WalkDir::new(dsl_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.path().extension().and_then(|s| s.to_str()) != Some("hbs") {
            continue;
        }
        if let Some(pat) = dev_fixture_pattern(&entry.path().display().to_string()) {
            matched.push((entry.path().to_path_buf(), pat));
            if matches!(action, DevFixtureAction::Refuse) {
                // Refuse fast on first match — no point walking the
                // whole tree once we know the boot will abort.
                break;
            }
        }
    }
    if matched.is_empty() {
        return Ok(());
    }
    match action {
        DevFixtureAction::Ignore => unreachable!(),
        DevFixtureAction::Warn => {
            for (path, pat) in &matched {
                tracing::warn!(
                    source_path = %path.display(),
                    pattern = pat,
                    "dsl: loading dev-fixture DSL (dev env permitted)",
                );
            }
            Ok(())
        }
        DevFixtureAction::Refuse => {
            let (path, pat) = &matched[0];
            Err(format!(
                "REFUSING TO LOAD dev-fixture DSL in non-dev environment: {} (matched pattern '{}'). \
                 Remove the file from the prod build OR set APP_ENV=dev to permit dev fixtures locally.",
                path.display(),
                pat,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_words_classify_as_dev() {
        for word in ["dev", "development", "local", "DEV", " Dev  "] {
            assert_eq!(Environment::from_env_str(word), Environment::Dev, "{word}");
        }
    }

    #[test]
    fn prod_words_classify_as_production() {
        for word in ["prod", "production", "live", "PROD"] {
            assert_eq!(
                Environment::from_env_str(word),
                Environment::Production,
                "{word}"
            );
        }
    }

    #[test]
    fn unknown_env_defaults_to_production_for_safety() {
        assert_eq!(Environment::from_env_str("banana"), Environment::Production);
        assert_eq!(
            Environment::from_env_str("prroduction"),
            Environment::Production
        );
    }

    #[test]
    fn enforce_posture_refuses_in_production() {
        let checks = vec![PostureCheck {
            name: "test.flag",
            is_safe: false,
            description: "some risk",
            fix: "flip to safe",
        }];
        let err = enforce_posture(Environment::Production, &checks).unwrap_err();
        assert!(err.contains("REFUSING TO START"));
        assert!(err.contains("test.flag"));
        assert!(err.contains("flip to safe"));
    }

    #[test]
    fn enforce_posture_accepts_in_dev() {
        let checks = vec![PostureCheck {
            name: "x",
            is_safe: false,
            description: "y",
            fix: "z",
        }];
        assert!(enforce_posture(Environment::Dev, &checks).is_ok());
    }

    #[test]
    fn enforce_posture_passes_when_all_safe() {
        let checks = vec![PostureCheck {
            name: "x",
            is_safe: true,
            description: "y",
            fix: "z",
        }];
        assert!(enforce_posture(Environment::Production, &checks).is_ok());
    }

    // ---------- §11.3 dev-fixture patterns ----------

    #[test]
    fn dev_fixture_pattern_matches_documented_families() {
        let cases = [
            ("/app/DSL/dev-login/session.hbs", "dev-login"),
            ("/app/DSL/mock-platform/health.hbs", "mock-"),
            ("/app/DSL/http/staging-mock/probe.hbs", "-mock"),
            ("/app/DSL/http/test/probe.hbs", "/test/"),
            ("/app/DSL/example-http/fetch.hbs", "example-"),
            ("/app/DSL/http/prod-example/fetch.hbs", "-example"),
            ("/app/DSL/dev/probe.hbs", "/dev/"),
            ("/app/DSL/mocks/probe.hbs", "/mocks/"),
        ];
        for (path, expected) in cases {
            assert_eq!(
                dev_fixture_pattern(path),
                Some(expected),
                "path {path} should match pattern {expected}"
            );
        }
    }

    #[test]
    fn dev_fixture_pattern_is_case_insensitive() {
        assert_eq!(
            dev_fixture_pattern("/app/DSL/MOCK-Foo/x.hbs"),
            Some("mock-")
        );
    }

    #[test]
    fn dev_fixture_pattern_normalises_windows_separators() {
        assert_eq!(
            dev_fixture_pattern("C:\\app\\DSL\\dev\\probe.hbs"),
            Some("/dev/")
        );
    }

    #[test]
    fn dev_fixture_pattern_leaves_shipped_samples_alone() {
        // The shipped `DSL/samples/` tree names must NOT trip any
        // pattern — they use unrelated words like `ping`, `echo`,
        // `flatten_address`, `map_products`.
        for path in [
            "/app/DSL/samples/ping.hbs",
            "/app/DSL/samples/echo.hbs",
            "/app/DSL/samples/arrays/map_products.hbs",
            "/app/DSL/samples/objects/select_fields.hbs",
            "/app/DSL/samples/conditionals/include_optional.hbs",
            "/app/DSL/samples/config/from_kv_array.hbs",
            "/app/DSL/samples/users/create.hbs",
            "/app/DSL/samples/users/patch.hbs",
            "/app/DSL/samples/strings/join_tags_csv.hbs",
            "/app/DSL/samples/transform/flatten_address.hbs",
            "/app/DSL/samples/advanced/nested_each_index.hbs",
            "/app/DSL/prod/webhook.hbs",
        ] {
            assert_eq!(dev_fixture_pattern(path), None, "path was: {path}");
        }
    }

    #[test]
    fn dev_fixture_action_from_env_matrix() {
        assert_eq!(
            DevFixtureAction::from_env(Environment::Production),
            DevFixtureAction::Refuse,
        );
        assert_eq!(
            DevFixtureAction::from_env(Environment::Staging),
            DevFixtureAction::Refuse,
        );
        assert_eq!(
            DevFixtureAction::from_env(Environment::Test),
            DevFixtureAction::Refuse,
        );
        assert_eq!(
            DevFixtureAction::from_env(Environment::Dev),
            DevFixtureAction::Warn,
        );
    }

    #[test]
    fn enforce_dev_fixture_scan_ignore_returns_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("mock-platform")).unwrap();
        std::fs::write(tmp.path().join("mock-platform/x.hbs"), "{}").unwrap();
        assert!(enforce_dev_fixture_scan(tmp.path(), DevFixtureAction::Ignore).is_ok());
    }

    #[test]
    fn enforce_dev_fixture_scan_refuses_in_prod() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("mock-platform")).unwrap();
        std::fs::write(tmp.path().join("mock-platform/x.hbs"), "{}").unwrap();
        let err = enforce_dev_fixture_scan(tmp.path(), DevFixtureAction::Refuse).unwrap_err();
        assert!(err.contains("REFUSING TO LOAD"));
        assert!(err.contains("mock-"));
        assert!(err.contains("APP_ENV=dev"));
    }

    #[test]
    fn enforce_dev_fixture_scan_warns_in_dev() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("mock-platform")).unwrap();
        std::fs::write(tmp.path().join("mock-platform/x.hbs"), "{}").unwrap();
        // Warn action returns Ok even when fixtures are present.
        assert!(enforce_dev_fixture_scan(tmp.path(), DevFixtureAction::Warn).is_ok());
    }

    #[test]
    fn enforce_dev_fixture_scan_shipped_samples_pass_refuse() {
        // A DSL tree containing only shipped-shape files (no
        // dev-fixture patterns) must load cleanly even under
        // Refuse.
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("samples/http")).unwrap();
        std::fs::write(tmp.path().join("samples/http/health.hbs"), "{}").unwrap();
        std::fs::write(tmp.path().join("samples/ping.hbs"), "{}").unwrap();
        assert!(enforce_dev_fixture_scan(tmp.path(), DevFixtureAction::Refuse).is_ok());
    }

    #[test]
    fn enforce_dev_fixture_scan_nonexistent_root_is_ok() {
        assert!(enforce_dev_fixture_scan(
            std::path::Path::new("/does/not/exist"),
            DevFixtureAction::Refuse
        )
        .is_ok());
    }
}
