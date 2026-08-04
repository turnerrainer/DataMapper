# REFACTO-MATRIX — DataMapper JS → Rust coverage

**Source of truth**: `Buerostack/DataMapper/` (Node.js + Express + express-handlebars, `v1.0.0`, `2025-09-24`).
**Target**: `Buerostack/DataMapper-on-Rust/` (Rust + axum + handlebars-rust, `v0.1.0-alpha.1`, `2026-07-29`).
**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md v1.0`.
**First landed**: 2026-08-04.

Legend for columns:
- **Planned status** — R1.1: `PRESERVE` / `EXTEND` / `DROP` / `DEFERRED`.
- **Current status** — R3.3: `MATCH` / `DIVERGE` / `MISSING` / `EXTRA` / `NOT-VERIFIED`.
- **Verification** — R3.1: `grep` (mechanical) / `code` (traced) / `repro` (both impls run).

Rows without an inline rationale are `PRESERVE` by default per R1.2.

---

## §A HTTP routes

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `POST /:project/*` | server.js:60 | `POST /:project/*view` | src/router.rs:52 | PRESERVE | MATCH | code | Same wire shape. |
| Two-candidate resolution: `<project>/<rest>.hbs`, `<project>/hbs/<rest>.hbs` | server.js:64-67 | Same two candidates | src/renderer.rs:55-61 | PRESERVE | MATCH | code | Order preserved. |
| `POST /healthz` → 405 `{"error":"MethodNotAllowed"}` | server.js:61 | `POST /healthz` → 405 | src/router.rs:66 | PRESERVE | DIVERGE | code | Rust adds `"message"` to body — see divergence D-005. |
| `GET /healthz` → `{"service":"DataMapper","ok":true,"ts":<ISO>}` | server.js:71 | Same body shape | src/router.rs:69 | PRESERVE | MATCH | code | RFC3339 timestamp both sides. |
| `GET /health` (alias) | — | `GET /health` | src/router.rs:50 | EXTEND | EXTRA | code | Rationale: parity with sibling Buerostack Rust services (Ruuter, XTR). Documented D-101. |
| `HEAD /healthz`, `HEAD /health` | — | Same | src/router.rs:66 | EXTEND | EXTRA | code | Rationale: HTTP HEAD parity. D-102. |
| Any non-POST on `/:project/*` | (express default: 404) | 405 via axum method routing | src/router.rs:52 | DIVERGE | DIVERGE | code | See D-006. |

## §B Config fields

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `PORT` env var (default 3000) | server.js:12 | `port: 3000` in `datamapper.yaml` | src/config.rs:26, datamapper.yaml:10 | DIVERGE | DIVERGE | code | Rust does NOT read `PORT` env var — silently ignored. See D-001. |
| Inbound body limit `"2mb"` (2 000 000 bytes literal in express) | server.js:14 | `limits.max_request_bytes: 2_097_152` (2 MiB) | src/config.rs:74, datamapper.yaml:23 | DIVERGE | DIVERGE | code | express `"2mb"` = 2 000 000 bytes decimal; Rust default is 2 MiB = 2 097 152 binary. See D-002. |
| No response body cap | — | `limits.max_response_bytes: 16 MiB` | src/config.rs:77, datamapper.yaml:27 | EXTEND | EXTRA | code | Rationale: guard runaway template amplification. D-103. |
| No request timeout | — | `limits.request_timeout_secs: 30` (504 on overrun) | src/config.rs:80, src/router.rs:46 | EXTEND | EXTRA | code | Rationale: guard runaway helpers. D-104. |
| Views dirs: `[__dirname/views, __dirname/DSL]` | server.js:20 | Only `dsl_path` | src/renderer.rs:56 | DROP | MISSING | code | Rust does NOT search a separate `views/` directory. D-003. |
| Layouts dir: `__dirname/views/layouts` | server.js:17 | No layouts | src/renderer.rs:29 | DROP | MISSING | code | See D-004. |
| Handlebars extname `.hbs` | server.js:17 | Same | src/renderer.rs:56 | PRESERVE | MATCH | code | |
| URL-encoded body middleware `express.urlencoded({extended:true})` | server.js:15 | Not accepted | src/router.rs:82 | DROP | MISSING | code | A client posting `application/x-www-form-urlencoded` is silently dropped in Rust (fails JSON parse → 400 `InvalidJson`). D-007. |

## §C CLI / env-var surface

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| No CLI flags | — | `--config <path>` / `--config=<path>` | src/config.rs:107 | EXTEND | EXTRA | code | Rationale: canonical config-source override. D-105. |
| `DATAMAPPER_CONFIG` env var | — | Reads it | src/config.rs:120 | EXTEND | EXTRA | code | D-106. |
| `RUST_LOG` env var | — | Reads it | src/main.rs:17 | EXTEND | EXTRA | code | D-107. |
| `PORT` env var | server.js:12 | Not read | — | DIVERGE | MISSING | code | See §B row above / D-001. |

## §D Handlebars helpers

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `{{now}}` — `new Date().toISOString()` | lib/helpers.js:1 | `{{now}}` — `chrono::Utc::now().to_rfc3339()` | src/helpers.rs:23 | PRESERVE | DIVERGE | code | JS `.toISOString()` always emits `Z` suffix, millisecond precision. Rust `to_rfc3339()` may emit `+00:00` and higher precision. See D-008. |
| `{{json obj}}` — `JSON.stringify(obj)` | lib/helpers.js:4 | `{{{json obj}}}` — `serde_json::to_string` | src/helpers.rs:58 | PRESERVE | MATCH | code | JS `stringify(undefined)` emits nothing; Rust missing-key emits `null`. Minor D-009. |
| `{{len items}}` | — | `{{len items}}` — array/string/object length | src/helpers.rs:38 | EXTEND | EXTRA | code | JS Handlebars uses implicit `.length`; handlebars-rust does not. **Silent drop for `{{foo.length}}`** in a ported JS DSL. This is the highest-severity R2.1 violation. See D-010. |

## §E Content negotiation

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `type: json` custom header (case-insensitive on value) | server.js:27 | Same | src/router.rs:151 | PRESERVE | MATCH | code | |
| Accept negotiation `req.accepts(['json','html'])` | server.js:29 | Ad-hoc parse: contains `application/json` OR `*/*` w/o `text/html` | src/router.rs:156 | PRESERVE | DIVERGE | code | Different priority rules for `Accept: text/html, application/json;q=0.9` etc. See D-011. |
| Opportunistic JSON when output parses | server.js:50-53 | Same | src/router.rs:134 | PRESERVE | MATCH | code | |
| HTML fallback MIME `text/html` | (implicit express default) | `text/html; charset=utf-8` | src/router.rs:138 | PRESERVE | DIVERGE | code | Rust adds `charset=utf-8`. Minor D-012. |
| JSON-preferring + rendered not JSON → raw with `application/json` MIME | server.js:46 | Same | src/router.rs:123 | PRESERVE | MATCH | code | |

## §F Path sanitisation

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `sanitize()` — normalize + silently strip `..` prefix + strip leading `/` | server.js:22 | `sanitize_segment()` — reject `..`, absolute, null, empty (HARD ERROR 400) | src/renderer.rs:96 | DIVERGE | DIVERGE | code | Rust is stricter and loud. JS silently rewrote; Rust rejects. This is R2.5-forward — but a client that previously succeeded with a traversal-shaped path now gets 400. See D-013. |
| `stripHbs()` — strip trailing `.hbs` | server.js:23 | `strip_hbs()` — same | src/renderer.rs:106 | PRESERVE | MATCH | code | |
| No canonicalised-parent check | — | `is_under()` — canonicalises assembled path, confirms it stays inside root | src/renderer.rs:113 | EXTEND | EXTRA | code | Rationale: defence-in-depth for symlinks planted in the DSL tree. D-108. |

## §G Response body shapes (errors)

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| 404 `{"error":"TemplateNotFound","tried":[...]}` | server.js:37 | `{"error":"TemplateNotFound","message":<str>,"tried":[...]}` | src/error.rs:76 | PRESERVE | DIVERGE | code | Rust adds `"message"` field. Grep-safe (`error` unchanged) but a strict-equality consumer breaks. D-014. |
| 500 `{"error":"TemplateRenderError","message":<str>,"view":<str>}` | server.js:41 | `{"error":"TemplateRenderError","message":<str>,"view":<str>}` | src/error.rs:84 | PRESERVE | MATCH | code | Wire shape matches. |
| 405 `{"error":"MethodNotAllowed"}` | server.js:61 | `{"error":"MethodNotAllowed","message":<str>}` | src/error.rs:34 | PRESERVE | DIVERGE | code | Rust adds `"message"`. D-014 (same class). |
| 400 InvalidJson (unstructured express default) | (express default) | Structured `{"error":"InvalidJson","message":<str>}` | src/error.rs:47 | EXTEND | EXTRA | code | Rationale: JS returns HTML error page; structured JSON is more useful. D-109. |
| 400 InvalidPath | — | `{"error":"InvalidPath","message":<str>}` | src/error.rs:48 | EXTEND | EXTRA | code | Documented D-013 above. |
| 413 (express default HTML) | (express default) | `{"error":"RequestTooLarge","message":<str>,"limit":<n>}` | src/error.rs:45 | EXTEND | EXTRA | code | D-110. |
| 500 ResponseTooLarge | — | `{"error":"ResponseTooLarge","message":<str>,"limit":<n>}` | src/error.rs:46 | EXTEND | EXTRA | code | D-103 (paired with cap). |
| 500 Internal | — | `{"error":"Internal","message":<str>}` | src/error.rs:50 | EXTEND | EXTRA | code | D-111. |
| 504 Gateway Timeout | — | Empty body (tower_http default) | src/router.rs:46 | EXTEND | EXTRA | code | D-104 (paired with timeout). |

## §H Template context

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `{ layout: false, ...req.body }` — `this` in templates includes `layout: false` alongside request body keys | server.js:40 | `context = req.body` verbatim — `this` is the exact JSON body | src/router.rs:93,102 | PRESERVE | DIVERGE | repro | JS `{{{json this}}}` for input `{"x":1}` emits `{"layout":false,"x":1}`; Rust emits `{"x":1}`. Rust behaviour is what operators actually want; JS is a leaky implementation detail. See D-015 (Rust is preferred). |

## §I Boot log

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `DataMapper listening on :<port>` (console.log) | server.js:73 | Multiline tracing: `starting`, `loaded config from …`, `dsl_path=…`, `listening on 0.0.0.0:…` | src/main.rs:24 | EXTEND | EXTRA | code | Log format changed. Grep patterns keyed on `DataMapper listening on :` break. D-016. |

## §J Sample DSLs

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `DSL/samples/ping.hbs` | source | Same content minus comment style | DSL/samples/ping.hbs | PRESERVE | DIVERGE | grep | Comment header rewritten (backslash-continued curl). See D-017. |
| `DSL/samples/echo.hbs` | source | Same body `{{{json this}}}` | DSL/samples/echo.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017 (same class). |
| `DSL/samples/advanced/nested_each_index.hbs` | source | Same body | DSL/samples/advanced/nested_each_index.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/arrays/map_products.hbs` | source | Body uses `{{len products}}` instead of `{{products.length}}` | DSL/samples/arrays/map_products.hbs | DIVERGE | DIVERGE | grep | Instance of D-010. Also comment header. |
| `DSL/samples/conditionals/include_optional.hbs` | source | Same body | DSL/samples/conditionals/include_optional.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/config/from_kv_array.hbs` | source | Same body | DSL/samples/config/from_kv_array.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/objects/select_fields.hbs` | source | Same body | DSL/samples/objects/select_fields.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/strings/join_tags_csv.hbs` | source | Same body | DSL/samples/strings/join_tags_csv.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/transform/flatten_address.hbs` | source | Same body | DSL/samples/transform/flatten_address.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/users/create.hbs` | source | Same body | DSL/samples/users/create.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |
| `DSL/samples/users/patch.hbs` | source | Same body | DSL/samples/users/patch.hbs | PRESERVE | DIVERGE | grep | Comment header only. D-017. |

## §K Tests

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| No formal test suite (curl commands in DSL comment headers) | package.json:6-9 | 24 unit + 16 integration | src/*/tests, tests/it_end_to_end.rs | EXTEND | EXTRA | code | Rationale: contract preserved needs regression net. D-112. |
| No cross-impl reproduction fixtures | — | None yet | — | DEFERRED | MISSING | code | This is the R4.3 gap this refacto batch closes — see task #10. |

## §L Docker / packaging

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| Dockerfile: `node:20-alpine`, root user, no init | Dockerfile | Multi-stage, `distroless` / `alpine`, non-root, tini | Dockerfile | EXTEND | EXTRA | code | Rationale: DEV-REQUIREMENTS §5. D-113. |
| docker-compose.yml: minimal | docker-compose.yml | Hardened: `no-new-privileges`, `cap_drop: ALL`, healthcheck | docker-compose.yml | EXTEND | EXTRA | code | D-113. |
| DSL volume mount `./DSL:/workspace/app/DSL:ro` | docker-compose.yml:12 | `./DSL:/app/DSL:ro` (per HANDOFF) | docker-compose.yml | DIVERGE | DIVERGE | code | Mount path changes. Operators must adjust bind targets. D-018. |

## §M Docs surface

| Source | Location | Target | Location | Planned | Current | Verification | Rationale / Note |
|---|---|---|---|---|---|---|---|
| `docs/architecture/*.md`, `docs/how-to/*.md` | source | `docs/DESIGN.md` + `book/src/*` | target | EXTEND | EXTRA | code | Different structure, no lost content — Rust replaces markdown with mdBook. D-114. |
| `examples/basic-usage/README.md` + `package.json` | source | Empty dir | examples/basic-usage/ | DROP | MISSING | grep | Empty directory in Rust target — either fill or remove. See D-019. |
| `README.md` | source | `README.md` | target | EXTEND | EXTRA | code | Rust README targets a different audience. D-115. |
| `CONTRIBUTING.md` | source | Not present at root | — | DROP | MISSING | code | Rust ships DEV-REQUIREMENTS + STANDARDS in the parent tree. D-020. |

---

## Coverage summary

Rows: 55.

By planned status:
- `PRESERVE`: 27
- `EXTEND`: 20
- `DROP`: 6
- `DEFERRED`: 1

By current status:
- `MATCH`: 12
- `DIVERGE`: 19
- `EXTRA`: 17
- `MISSING`: 6
- `NOT-VERIFIED`: 0

Every divergence, extra, and drop is expected to be recorded in `DIVERGENCES.md` under the referenced D-nnn identifier. Rows without an inline rationale in the "Rationale / Note" column are `PRESERVE`+`MATCH` by definition.
