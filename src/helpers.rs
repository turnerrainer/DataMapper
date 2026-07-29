//! Built-in Handlebars helpers.
//!
//! Registered on the shared `handlebars::Handlebars` instance in
//! `renderer.rs`. Each helper here is safe to invoke from any DSL
//! and has no side effects.
//!
//! Adding a helper: implement `HelperDef`, register it in
//! [`register_all`], document it in `book/src/configuration.md`
//! under "Built-in helpers", and add a template + integration test
//! that exercises it end-to-end.

use handlebars::{
    Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderErrorReason,
};

pub fn register_all(reg: &mut Handlebars<'_>) {
    reg.register_helper("now", Box::new(now_helper));
    reg.register_helper("json", Box::new(json_helper));
    reg.register_helper("len", Box::new(len_helper));
}

/// `{{now}}` → current UTC time in RFC 3339 / ISO 8601.
fn now_helper(
    _: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: &mut dyn Output,
) -> HelperResult {
    out.write(&chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

/// `{{len items}}` → length of an array, string, or object; `0` for
/// null / missing. JS Handlebars resolves `foo.length` via JS's
/// array `.length` property; handlebars-rust has no equivalent
/// implicit accessor, so DSLs use `{{len foo}}` for the same effect.
fn len_helper(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: &mut dyn Output,
) -> HelperResult {
    let n = match h.param(0).map(|p| p.value()) {
        Some(serde_json::Value::Array(a)) => a.len(),
        Some(serde_json::Value::String(s)) => s.chars().count(),
        Some(serde_json::Value::Object(o)) => o.len(),
        _ => 0,
    };
    out.write(&n.to_string())?;
    Ok(())
}

/// `{{{json obj}}}` → serialise the parameter as compact JSON.
/// Use triple-brace so the output is emitted raw; double-brace
/// would HTML-escape quotes and break downstream JSON parsers.
fn json_helper(
    h: &Helper<'_>,
    _: &Handlebars<'_>,
    _: &Context,
    _: &mut RenderContext<'_, '_>,
    out: &mut dyn Output,
) -> HelperResult {
    let param = h
        .param(0)
        .ok_or_else(|| RenderErrorReason::Other("json helper requires 1 parameter".into()))?;
    let s = serde_json::to_string(param.value())
        .map_err(|e| RenderErrorReason::Other(format!("json serialise failed: {e}")))?;
    out.write(&s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn new_reg() -> Handlebars<'static> {
        let mut reg = Handlebars::new();
        reg.set_strict_mode(false);
        register_all(&mut reg);
        reg
    }

    #[test]
    fn now_emits_rfc3339() {
        let reg = new_reg();
        let out = reg.render_template("{{now}}", &json!({})).unwrap();
        // RFC 3339 timestamps start with year (4 digits) then '-'.
        assert!(out.len() >= 20);
        assert_eq!(&out[4..5], "-");
        assert_eq!(&out[7..8], "-");
    }

    #[test]
    fn json_serialises_object_compactly() {
        let reg = new_reg();
        let out = reg
            .render_template("{{{json obj}}}", &json!({ "obj": {"a": 1, "b": "x"} }))
            .unwrap();
        assert!(out.contains("\"a\":1"));
        assert!(out.contains("\"b\":\"x\""));
    }

    #[test]
    fn json_serialises_array() {
        let reg = new_reg();
        let out = reg
            .render_template("{{{json arr}}}", &json!({ "arr": [1, 2, 3] }))
            .unwrap();
        assert_eq!(out, "[1,2,3]");
    }

    #[test]
    fn len_of_array() {
        let reg = new_reg();
        let out = reg
            .render_template("{{len xs}}", &json!({ "xs": [1, 2, 3, 4] }))
            .unwrap();
        assert_eq!(out, "4");
    }

    #[test]
    fn len_of_string() {
        let reg = new_reg();
        let out = reg
            .render_template("{{len s}}", &json!({ "s": "abcde" }))
            .unwrap();
        assert_eq!(out, "5");
    }

    #[test]
    fn len_of_missing_is_zero() {
        let reg = new_reg();
        let out = reg.render_template("{{len missing}}", &json!({})).unwrap();
        assert_eq!(out, "0");
    }

    #[test]
    fn len_of_object() {
        let reg = new_reg();
        let out = reg
            .render_template("{{len obj}}", &json!({ "obj": { "a": 1, "b": 2 } }))
            .unwrap();
        assert_eq!(out, "2");
    }

    #[test]
    fn json_missing_param_errors() {
        let reg = new_reg();
        let err = reg.render_template("{{{json}}}", &json!({})).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("json helper"));
    }
}
