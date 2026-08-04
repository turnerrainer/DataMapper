# REFACTO-AUDIT §2 — contract preservation

**Written**: 2026-08-04.
**Scope**: audit `DataMapper-on-Rust/` against `Buerostack/REFACTO-REQUIREMENTS.md §2` (silent drops, unknown fields, defaults, aliases, error strings), using the JS source of truth (`Buerostack/DataMapper/`) as the reference.
**Method**: R3.1 verification levels attached to every finding. Findings acted on (fix landed) are lifted to `(reproduced)` by task #6 landing regression fixtures.

Findings are numbered `F-nn`. Each maps to a divergence entry
(`DIVERGENCES.md` D-nnn) and, where a fix is required, to a
regression test filename.

---

## §R2.1 — Silent drops (FORBIDDEN)

### F-01. `PORT` environment variable silently ignored

- **JS behaviour**: `server.js:12` — `const PORT = process.env.PORT || 3000;`.
  Setting `PORT=8080` binds on 8080.
- **Rust behaviour** (code-read): `src/config.rs:68-70` defines `default_port() -> 3000`; `src/config.rs:107` reads `--config` CLI flag and `DATAMAPPER_CONFIG` env var, but never `PORT`. An operator who sets `PORT=8080` (as documented in the JS README) gets `port=3000` silently.
- **Requirement**: R2.1 — the target MUST either honour, reject, or WARN about the input.
- **Severity**: HIGH. Direct porting of docker-compose / systemd unit files from JS DataMapper breaks silently.
- **Verification**: grep-verified (no `env::var("PORT")` anywhere under `src/`).
- **Divergence**: D-001.
- **Fix**: honour `PORT` env var as fallback when `port` is not set in `datamapper.yaml`.
- **Regression fixture**: `tests/it_regression_refacto.rs::port_env_var_is_honoured_when_config_absent`.

### F-02. `{{foo.length}}` in a ported JS DSL silently renders as empty string

- **JS behaviour**: JS Handlebars resolves `{{arr.length}}` via the JS array's `.length` property (implicit). Templates in the wild (`arrays/map_products.hbs` in the JS source) use it: `"total": {{#if products}}{{products.length}}{{else}}0{{/if}}`.
- **Rust behaviour** (code-read): `handlebars-rust` has no implicit `.length` accessor. `set_strict_mode(false)` (`src/renderer.rs:34`) makes missing-property lookups render empty. So `{{products.length}}` renders `""` — leaving `"total": {{#if products}}{{else}}0{{/if}}` → `"total": ` (invalid JSON) or `"total": 0`. No warn, no error.
- **Requirement**: R2.1.
- **Severity**: CRITICAL. This is the exact "silent drop" pattern the requirement calls out — accepts input the source used to react to, produces no visible effect, no diagnostic.
- **Verification**: grep-verified in shipped Rust `arrays/map_products.hbs` (author already rewrote it to use `{{len products}}`, proving they knew this fractures). Code-read for the general case.
- **Divergence**: D-010.
- **Fix**: detect `.length` accessors at template-load time and either:
  1. Register a Handlebars helper-shim so `foo.length` resolves through a preprocessor, OR
  2. Emit an `error` at render (best) or `warn!` at first render with a clear pointer to `{{len foo}}` and the MIGRATION.md entry.
- **Choice for this batch**: fix option (2) — a `warn!` on first render of a template containing `.length`, plus a hard `TemplateRenderError` when strict-`.length` mode is enabled via config. Keeping the default at "warn, don't fail" preserves existing Rust DSLs; a future release (per R6.3 grace window) can flip to hard reject.
- **Regression fixture**: `tests/it_regression_refacto.rs::dot_length_in_ported_js_dsl_warns_and_renders_empty`.

### F-03. `application/x-www-form-urlencoded` request bodies silently rejected as `InvalidJson`

- **JS behaviour**: `server.js:15` — `app.use(express.urlencoded({ extended: true }));`. Form-encoded bodies are parsed into `req.body` as a plain object; templates render.
- **Rust behaviour** (code-read): `src/router.rs:82-100` — treats the raw body as JSON, so `key=val&foo=bar` returns 400 `{"error":"InvalidJson","message":"expected value"}`. Message doesn't hint at the Content-Type mismatch.
- **Requirement**: R2.1 — this is loud (400), but the error is misleading (message says "not valid JSON" without pointing at the Content-Type gap).
- **Severity**: MEDIUM. Not a true silent drop — it's a loud-but-wrong error. Still a §2.5 concern (message drift).
- **Verification**: code-read.
- **Divergence**: D-007.
- **Fix**: when `Content-Type` is `application/x-www-form-urlencoded`, either (a) decode it as JSON-compatible key/value pairs, or (b) return a **specific** error `UnsupportedContentType` naming the type and pointing at the migration. Choice for this batch: option (b) — cleanest, respects R2.5. Keeps the "JSON-in" contract clear.
- **Regression fixture**: `tests/it_regression_refacto.rs::form_urlencoded_returns_unsupported_content_type`.

### F-04. `views/` root and `layouts/` subdirectory silently absent

- **JS behaviour**: `server.js:17,20` — engine's `layoutsDir` is `<repo>/views/layouts`; view roots are `[<repo>/views, <repo>/DSL]`. A user template placed under `views/` resolves; layouts referenced via express-handlebars `{{#extend "foo"}}` resolve.
- **Rust behaviour** (code-read): `src/renderer.rs:56` — only `dsl_root` is searched. `views/` is a silent no-op. Template files placed there are 404'd.
- **Requirement**: R2.1.
- **Severity**: LOW. `views/` is not documented as a user-facing input in JS README or docs; only server.js references it. But R2.1 is unconditional on "input the source used to react to."
- **Verification**: grep-verified (no `views/` in Rust code except this test).
- **Divergence**: D-003, D-004.
- **Fix**: at boot, if `<repo>/views/` exists and has `.hbs` files, emit a `warn!` naming the directory and pointing at the migration (either move the files under `dsl_path` or drop them). Keeps Rust's simpler search tree, honours R2.1.
- **Regression fixture**: `tests/it_regression_refacto.rs::views_dir_with_hbs_files_warns_at_boot`.

## §R2.2 — Unknown-field tolerance in user-authored formats

### F-05. `AppConfig` accepts unknown top-level YAML fields silently

- **Rust behaviour** (code-read): `src/config.rs:24-34` — `#[derive(Deserialize)]` on `AppConfig` without `#[serde(deny_unknown_fields)]`. A typo (`dsl-path:` instead of `dsl_path:`) parses cleanly, defaults kick in, operator has no idea their config didn't take effect.
- **Requirement**: R2.2 (and R6.1 — `deny_unknown_fields` REQUIRED at top-level and every nested struct).
- **Severity**: HIGH — classic silent-config bug.
- **Verification**: grep-verified (`git grep 'deny_unknown_fields' src/` returns zero hits).
- **Divergence**: none — this is a strict R6.1 violation. Fixed here.
- **Fix**: add `#[serde(deny_unknown_fields)]` on `AppConfig` and `Limits`.
- **Regression fixture**: `it_regression_refacto::unknown_config_field_hard_fails_at_parse`.

## §R2.3 — Default drift

### F-06. `max_request_bytes` default drifts from JS

- **JS behaviour**: `server.js:14` — `express.json({ limit: "2mb" })`. Express's `bytes` package parses `"2mb"` as **2 000 000 bytes** (decimal SI-style).
- **Rust behaviour** (code-read): `src/config.rs:74` — `2 * 1024 * 1024 = 2 097 152` bytes (binary MiB).
- **Requirement**: R2.3.
- **Severity**: MEDIUM. In practice a client at 2 000 001 bytes passes on Rust but fails on JS; almost nobody in production, but for R2.3 correctness it matters.
- **Verification**: code-read on both sides. `bytes` package (v3.x) parses `"2mb"` as 2 000 000; docs at https://www.npmjs.com/package/bytes.
- **Divergence**: D-002.
- **Choice for this batch**: **DOCUMENT the divergence** (keep 2 MiB for consistency with binary SI, matches operators' intuition), don't silently change it. Add a startup INFO note per R2.3 second sentence.
- **Regression fixture**: `it_regression_refacto::request_limit_default_is_2_mib_binary`.

### F-07. `port` default matches (3000 both sides)

- No drift. `(reproduced)` by ping smoke.

### F-08. Response cap + timeout are NEW defaults (no JS equivalent)

- Extension, not drift. Recorded D-103, D-104. Both emit their limits at boot (`src/main.rs:32-37`).

## §R2.4 — Field-name aliases

### F-09. JS accepts no field aliases; nothing to preserve

- **JS behaviour**: `server.js` reads `PORT` env only; no YAML config. No aliases exist.
- **Verification**: grep-verified.
- **Result**: N/A. No fix.

## §R2.5 — Error-string parity

### F-10. Error responses gain a `"message"` field

- **JS behaviour**: `server.js:37,41,61` — errors are `{"error":"…"}` (plus `tried`/`view`/`message` on specific variants).
- **Rust behaviour**: `src/error.rs:71-89` — every error carries `"error"` + `"message"`, plus per-variant extras. `"message"` is derived from `thiserror::Display`.
- **Requirement**: R2.5 — "MUST emit either the same message or a message that names the same underlying condition."
- **Severity**: LOW. The `error` field (grep target) is preserved verbatim. `message` is additive; log-grep patterns keyed on `"error":"TemplateNotFound"` still work.
- **Verification**: code-read plus test `it_end_to_end::template_not_found_returns_404` and `healthz_post_is_method_not_allowed`.
- **Divergence**: D-014.
- **Choice**: keep the extra `message`; documented, not a fix.

### F-11. Boot log format drift

- **JS behaviour**: `server.js:73` — single line `DataMapper listening on :3000`.
- **Rust behaviour**: `src/main.rs:24,28-37,47` — multi-line tracing output; no single line matches the JS grep pattern.
- **Requirement**: R2.5 — messages users have dashboards keyed on.
- **Severity**: MEDIUM. Operators with log-grep monitors on `DataMapper listening on :` break silently.
- **Verification**: code-read.
- **Divergence**: D-016.
- **Fix**: emit a **single** back-compat log line `DataMapper listening on :<port>` alongside the structured tracing output. Keeps every JS-derived monitor working; costs one println. See D-016.
- **Regression fixture**: `it_regression_refacto::boot_emits_js_compatible_listening_line`.

### F-12. `healthz` body shape

- Both sides: `{"service":"DataMapper","ok":true,"ts":"<ISO>"}`. `(reproduced)` by ping smoke.

## Summary

| Class | Findings | Fixes landed in this batch | Documented as divergence |
|---|---|---|---|
| R2.1 silent drop | 4 (F-01..F-04) | 4 | 4 (D-001, D-003/004, D-007, D-010) |
| R2.2 unknown fields | 1 (F-05) | 1 | 0 (strict compliance) |
| R2.3 default drift | 3 (F-06..F-08) | 1 doc-only | 2 (D-002, D-103/104) |
| R2.4 alias loss | 0 | — | — |
| R2.5 error/log strings | 3 (F-10..F-12) | 1 boot-log restore | 2 (D-014, D-016) |
| **Total** | **11 non-trivial** | **7 code fixes + 4 doc entries** | **8 divergences** |

## What this audit did NOT cover (R3.4)

- **Template context equality** — F not yet numbered. Whether express-handlebars strips `layout: false` from the render context before passing to Handlebars is `code-read-verified` from documentation, not `(reproduced)`. See §H of `REFACTO-MATRIX.md` and D-015. A cross-impl fixture in `compat/repro/` closes this in task #10.
- **Timestamp exact-format parity** — F-08 companion. JS `.toISOString()` always emits `Z` suffix, ms precision; Rust `chrono::to_rfc3339()` emits `+00:00` and higher precision. Grep monitors keyed on `Z"` break. Not fixed in this batch — logged as D-008 for a follow-up patch.
- **Handlebars engine differences beyond `.length`** — sparse-array indexing, prototype-chain lookups, block-helper argument coercion. Assumed compatible under the "trust the folder-drop input" contract; a future `handlebars-rust` upgrade could change this.
- **Docker mount path change** (`/workspace/app/DSL` → `/app/DSL`) — D-018. Documented, not fixed.
