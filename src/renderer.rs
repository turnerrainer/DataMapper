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

        let raw = std::fs::read_to_string(&found)
            .map_err(|e| DataMapperError::Internal(format!("reading template: {e}")))?;

        // R2.1 / D-010: JS Handlebars resolves `{{foo.length}}` via
        // the JS Array `.length` property; handlebars-rust does not.
        // Silently rendering empty (or 500-ing on subscript-into-array)
        // is the R2.1 pattern the REFACTO requirement calls out. We
        // honour the JS semantic by rewriting `.length` accessors to
        // the `len` helper before render (source-compatible), and
        // emit a `warn!` naming the template so operators can update
        // the DSL at their leisure (see MIGRATION.md §.length).
        let body = if contains_dot_length_accessor(&raw) {
            tracing::warn!(
                "template {} uses `.length` accessor; auto-rewriting to `(len …)` for compat with the JS DataMapper DSL — please migrate the template body (see DIVERGENCES.md D-010, MIGRATION.md §.length)",
                view_label
            );
            rewrite_dot_length(&raw)
        } else {
            raw
        };

        self.reg
            .render_template(&body, context)
            .map_err(|e| DataMapperError::TemplateRenderError {
                view: view_label,
                message: e.to_string(),
            })
    }
}

/// Rewrite `{{ path.length }}`, `{{{ path.length }}}`, and block
/// helpers like `{{#if path.length}}` / `{{#unless path.length}}` to
/// their `len`-helper equivalents. Comment blocks (`{{! ... }}` and
/// `{{!-- ... --}}`) are skipped. `path` may contain segment
/// components like `foo.[0].bar` (Handlebars index syntax).
pub fn rewrite_dot_length(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the matching closing `}}` (or `}}}` for triple).
            let triple = i + 2 < bytes.len() && bytes[i + 2] == b'{';
            let open_end = if triple { i + 3 } else { i + 2 };
            // Handle comment blocks — copy verbatim.
            if open_end < bytes.len() && bytes[open_end] == b'!' {
                let close = find_close(bytes, open_end);
                out.push_str(&template[i..close]);
                i = close;
                continue;
            }
            let close = find_close(bytes, open_end);
            let inner = &template[open_end..close.saturating_sub(if triple { 3 } else { 2 })];
            let rewritten = rewrite_dot_length_inner(inner);
            out.push_str(if triple { "{{{" } else { "{{" });
            out.push_str(&rewritten);
            out.push_str(if triple { "}}}" } else { "}}" });
            i = close;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn find_close(bytes: &[u8], start: usize) -> usize {
    let mut j = start;
    while j + 1 < bytes.len() {
        if bytes[j] == b'}' && bytes[j + 1] == b'}' {
            // Include the closing braces (2 or 3).
            if j + 2 < bytes.len() && bytes[j + 2] == b'}' {
                return j + 3;
            }
            return j + 2;
        }
        j += 1;
    }
    bytes.len()
}

/// Rewrite the inner (brace-less) body of a mustache expression.
///
/// Two shapes get rewritten:
///
/// 1. Value-position: `{{path.length}}` / `{{{path.length}}}` →
///    `{{len path}}`. JS Handlebars would emit the number; the `len`
///    helper does the same.
/// 2. Block-condition: `{{#if path.length}}` / `{{#unless path.length}}` →
///    `{{#if path}}` / `{{#unless path}}`. JS `if arr.length` is
///    truthy iff `arr.length > 0`, which for arrays and strings is
///    exactly the truthiness of `arr` itself in handlebars-rust.
///    Rewriting to `(len path)` would be wrong because
///    handlebars-rust treats the integer `0` as truthy.
///
/// Anything else that mentions `.length` is left as-is; the render
/// layer will surface the mismatch.
fn rewrite_dot_length_inner(inner: &str) -> String {
    let trimmed = inner.trim();
    // Simple value case: bare `path.length` (possibly with whitespace).
    if let Some(path) = trimmed.strip_suffix(".length") {
        if !path.is_empty() && is_bare_path(path) {
            return format!(" len {} ", path);
        }
    }
    // Block-opener cases like `#if path.length`, `#unless path.length`.
    // Rewrite to `#if path` — `arr.length > 0` ⟺ `arr` non-empty, and
    // handlebars-rust's `#if` treats empty arrays / strings as falsy.
    for prefix in ["#if ", "#unless ", "else if ", "if ", "unless "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            if let Some(path) = rest.strip_suffix(".length") {
                if !path.is_empty() && is_bare_path(path) {
                    return format!("{}{}", prefix, path);
                }
            }
        }
    }
    inner.to_string()
}

/// A Handlebars path segment: identifier chars, `.`, `[`, `]`, digits,
/// slashes (for relative-path scoping like `../foo`). Reject anything
/// else so we don't accidentally rewrite arbitrary subexpressions.
fn is_bare_path(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | '/' | '@'))
}

/// Match `{{...foo.length...}}` and `{{{...foo.length...}}}` mustache
/// blocks whose body ends in `.length` (possibly followed by more
/// identifier chars — we only warn on the exact `.length` token). We
/// scan mustache blocks rather than the whole file to skip
/// `{{!-- ... .length ... --}}` comment blocks.
pub fn contains_dot_length_accessor(template: &str) -> bool {
    let bytes = template.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let mut j = i + 2;
            // Skip triple-brace / comment / raw-block markers.
            if j < bytes.len() && (bytes[j] == b'!' || bytes[j] == b'{') {
                // Comments `{{! ... }}` and `{{!-- ... --}}` are
                // ignored. Triple-brace `{{{ ... }}}` we still scan.
                if bytes[j] == b'!' {
                    // find the closing `}}` — comment; skip.
                    while j + 1 < bytes.len() && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                        j += 1;
                    }
                    i = j + 2;
                    continue;
                }
            }
            // Scan mustache body up to closing braces.
            let start = j;
            while j + 1 < bytes.len() && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            let body = &template[start..j];
            if has_dot_length_token(body) {
                return true;
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
    false
}

fn has_dot_length_token(body: &str) -> bool {
    // Match `.length` where `length` is a full identifier terminator
    // (not `.lengthX` or `.length_x`). Split-and-check is simpler
    // than pulling in `regex`.
    let mut rest = body;
    while let Some(pos) = rest.find(".length") {
        let after = &rest[pos + ".length".len()..];
        let next = after.chars().next();
        // If the char right after `.length` continues an identifier,
        // it's a different token — skip.
        let ends = match next {
            None => true,
            Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
        };
        if ends {
            return true;
        }
        rest = after;
    }
    false
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
