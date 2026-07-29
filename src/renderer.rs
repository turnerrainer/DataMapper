//! Handlebars template resolution + rendering.
//!
//! `Renderer` owns a `handlebars::Handlebars` instance with all
//! built-in helpers registered, and a `dsl_root` path. It resolves
//! logical template names ("<project>/<view>") to `.hbs` files under
//! that root and renders them against a per-request context.
//!
//! Two lookup candidates are tried in order (mirrors the original
//! DataMapper Node.js behaviour so existing DSL trees migrate as-is):
//!   1. `<dsl_root>/<project>/<view>.hbs`
//!   2. `<dsl_root>/<project>/hbs/<view>.hbs`
//!
//! Path safety: `<project>` and `<view>` are sanitised before any
//! filesystem access — see [`sanitize_segment`].

use crate::error::DataMapperError;
use crate::helpers;
use handlebars::Handlebars;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Renderer {
    reg: Handlebars<'static>,
    dsl_root: PathBuf,
}

impl Renderer {
    pub fn new(dsl_root: PathBuf) -> Self {
        let mut reg = Handlebars::new();
        // Non-strict for wire-compat with the original DataMapper
        // Node.js server (JS handlebars is non-strict by default and
        // shipped DSLs depend on missing-key → empty-string). Guard
        // optional fields at the DSL layer with `{{#if}}` / `{{#unless}}`.
        reg.set_strict_mode(false);
        helpers::register_all(&mut reg);
        Self { reg, dsl_root }
    }

    /// Resolve `<project>/<view>` under the DSL root, render against
    /// `context`, and return the rendered string.
    ///
    /// Returns:
    /// * `TemplateNotFound` if neither candidate exists on disk.
    /// * `InvalidPath` if any segment fails traversal guards.
    /// * `TemplateRenderError` if Handlebars fails to render.
    pub fn render(
        &self,
        project: &str,
        view: &str,
        context: &Value,
    ) -> Result<String, DataMapperError> {
        let project = sanitize_segment(project)?;
        let view = sanitize_segment(&strip_hbs(view))?;

        let candidates = [
            self.dsl_root.join(&project).join(format!("{view}.hbs")),
            self.dsl_root
                .join(&project)
                .join("hbs")
                .join(format!("{view}.hbs")),
        ];

        let logical_tried: Vec<String> = candidates
            .iter()
            .map(|p| {
                p.strip_prefix(&self.dsl_root)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect();

        let (found, view_label) = candidates
            .iter()
            .zip(logical_tried.iter())
            .find(|(p, _)| is_under(p, &self.dsl_root) && p.is_file())
            .map(|(p, label)| (p.clone(), label.clone()))
            .ok_or(DataMapperError::TemplateNotFound {
                tried: logical_tried.clone(),
            })?;

        let body = std::fs::read_to_string(&found)
            .map_err(|e| DataMapperError::Internal(format!("reading template: {e}")))?;

        self.reg
            .render_template(&body, context)
            .map_err(|e| DataMapperError::TemplateRenderError {
                view: view_label,
                message: e.to_string(),
            })
    }
}

/// Reject empty, absolute, or traversal-carrying segments. Called on
/// every URL-derived path piece before it touches the filesystem.
pub fn sanitize_segment(s: &str) -> Result<String, DataMapperError> {
    if s.is_empty() {
        return Err(DataMapperError::InvalidPath("empty segment".into()));
    }
    if s.contains("..") || s.contains('\0') || s.starts_with('/') || s.starts_with('\\') {
        return Err(DataMapperError::InvalidPath(s.into()));
    }
    Ok(s.to_string())
}

fn strip_hbs(s: &str) -> String {
    s.strip_suffix(".hbs").unwrap_or(s).to_string()
}

/// Defence-in-depth: after path assembly, confirm the resolved path
/// stays under `root`. Catches any traversal that slipped past
/// `sanitize_segment` — e.g., a symlink inside the DSL tree.
fn is_under(path: &Path, root: &Path) -> bool {
    let (Ok(p), Ok(r)) = (path.canonicalize(), root.canonicalize()) else {
        // Path doesn't exist yet or can't be canonicalised — fall
        // back to a lexical check on the pre-canonical parent.
        return path.starts_with(root);
    };
    p.starts_with(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(sanitize_segment("..").is_err());
        assert!(sanitize_segment("foo/../bar").is_err());
        assert!(sanitize_segment("../secrets").is_err());
    }

    #[test]
    fn sanitize_rejects_null_byte() {
        assert!(sanitize_segment("foo\0bar").is_err());
    }

    #[test]
    fn sanitize_rejects_absolute() {
        assert!(sanitize_segment("/etc/passwd").is_err());
        assert!(sanitize_segment("\\windows").is_err());
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(sanitize_segment("").is_err());
    }

    #[test]
    fn sanitize_accepts_normal_names() {
        assert_eq!(sanitize_segment("users").unwrap(), "users");
        assert_eq!(
            sanitize_segment("create-user_v2").unwrap(),
            "create-user_v2"
        );
    }

    #[test]
    fn strip_hbs_removes_suffix() {
        assert_eq!(strip_hbs("foo.hbs"), "foo");
        assert_eq!(strip_hbs("foo"), "foo");
        assert_eq!(strip_hbs("bar.hbs.hbs"), "bar.hbs");
    }
}
