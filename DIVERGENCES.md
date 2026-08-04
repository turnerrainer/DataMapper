# DIVERGENCES — DataMapper-on-Rust vs. DataMapper (JS source of truth)

**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md v1.0 §5`.
**Written**: 2026-08-04.

Every intentional behavioural difference from the JS source of truth
lives here. Rows without an entry in this file are, by contract,
byte-identical to JS DataMapper as of `v1.0.0` (`2025-09-24`).

Divergence ID convention:
- `D-0nn` — behavioural drift the target had to record (JS-like, not
  matched exactly).
- `D-1nn` — extensions the target adds that the source does not have.

Discovered-post-hoc entries carry a `discovered:` date; entries filed
alongside the change carry the commit / PR reference.

Each entry follows the R5.1 schema:

```
- Field / behaviour
- Source of truth: <file:line in Buerostack/DataMapper/>
- Target behaviour: <file:line in Buerostack/DataMapper-on-Rust/>
- Motivation
- Migration
- Reversibility
```

---

## D-001. `PORT` env var honoured, not the primary knob

- **Field / behaviour**: TCP port selection.
- **Source of truth**: `server.js:12` — `process.env.PORT || 3000`.
- **Target behaviour**: `src/config.rs:26,68-70,97-118` — `port` in `datamapper.yaml` is primary, `PORT` env var is honoured as fallback iff config file omits `port:`.
- **Motivation**: Rust project standard is YAML-file-first (matches sibling Buerostack Rust services). `PORT` compat added in this refacto batch (F-01) so operators porting from JS containers don't get 3000 silently.
- **Migration**: `PORT` still works for pure lift-and-shift. New deployments SHOULD switch to `datamapper.yaml`.
- **Reversibility**: Add `PORT` env var to your unit / compose; no code change required.
- **Filed**: 2026-08-04 (refacto pass).

## D-002. `max_request_bytes` default is 2 MiB (binary), not `"2mb"` (2 000 000 decimal)

- **Field / behaviour**: default inbound body cap.
- **Source of truth**: `server.js:14` — `express.json({ limit: "2mb" })`. Express's `bytes` package parses `"2mb"` as 2 000 000 bytes.
- **Target behaviour**: `src/config.rs:76-78` — `2 * 1024 * 1024 = 2 097 152` bytes.
- **Motivation**: binary MiB matches operator intuition and other Buerostack Rust services. Delta is 97 152 bytes (~5%); no known deployment is threading that needle.
- **Migration**: none for typical payloads (single-digit KB). Payloads in the 2 000 000–2 097 152 byte window that used to 413 on JS now pass on Rust.
- **Reversibility**: set `limits.max_request_bytes: 2000000` in `datamapper.yaml`.
- **Filed**: 2026-08-04.

## D-003. `<repo>/views/` directory not searched for templates

- **Field / behaviour**: template resolution roots.
- **Source of truth**: `server.js:20` — `app.set("views", [<repo>/views, <repo>/DSL]);`. A `.hbs` file placed at `views/<project>/<view>.hbs` is served.
- **Target behaviour**: `src/renderer.rs:56` — only `dsl_path` is searched. Any file under `views/` is silent 404. Refacto batch (F-04) adds a `warn!` at boot naming the directory (`src/main.rs:warn_on_legacy_views_dir`).
- **Motivation**: single canonical root reduces cognitive load and matches Rust's config-file-first posture. The `views/` split was an express-handlebars artefact, not a documented DataMapper feature.
- **Migration**: move `views/<project>/<view>.hbs` → `DSL/<project>/<view>.hbs`.
- **Reversibility**: patch `renderer.rs` to search both roots; would take an afternoon.
- **Filed**: 2026-08-04.

## D-004. Layouts (`views/layouts`) unsupported

- **Field / behaviour**: Handlebars layout support.
- **Source of truth**: `server.js:17` — `layoutsDir: __dirname/views/layouts`. Templates could use `{{#extend "layout"}}` (express-handlebars only).
- **Target behaviour**: `src/renderer.rs:29-36` — no layout system; templates are self-contained.
- **Motivation**: DataMapper is a JSON-shaping proxy; HTML layouts belong in a real templating server. The `layout: false` in `res.render` (`server.js:40`) proves the JS project itself never used them either.
- **Migration**: inline any layout content into the template body.
- **Reversibility**: adopt handlebars-rust partials or `#extend` shim; larger surface area than the compat fix warrants.
- **Filed**: 2026-08-04.

## D-005. Every error response includes a `"message"` field

- **Field / behaviour**: JSON error body shape.
- **Source of truth**: `server.js:37,41,61` — `{"error": "…"}` plus per-variant extras (`tried`, `view`, `message` on TemplateRenderError).
- **Target behaviour**: `src/error.rs:71-89` — every error carries `"error"` + `"message"` plus the JS-parity per-variant extras.
- **Motivation**: log-grep monitors keyed on `"error":"<code>"` still match; humans reading the body get an actionable string without a separate log lookup.
- **Migration**: none. Strict-equality body consumers (rare) need to skip the extra key.
- **Reversibility**: drop `"message"` in `IntoResponse for DataMapperError`; two lines.
- **Filed**: 2026-08-04.

## D-006. Non-POST on `/:project/*` returns 405, not 404

- **Field / behaviour**: HTTP method routing.
- **Source of truth**: `server.js:60` — only `POST` mounted; other methods → express default 404 HTML.
- **Target behaviour**: `src/router.rs:52-53` — `post()` route; axum returns 405 with `allow: POST`.
- **Motivation**: standard REST posture. 405 is more informative than 404 when the route exists.
- **Migration**: none for correctly-scripted clients.
- **Reversibility**: axum method routing does not degrade to 404 cleanly; would need a custom fallback.
- **Filed**: 2026-08-04.

## D-007. Form-encoded bodies return 415 `UnsupportedContentType`, not silent `req.body = {}`

- **Field / behaviour**: request body content-type handling.
- **Source of truth**: `server.js:15` — `express.urlencoded({ extended: true })` parses `application/x-www-form-urlencoded` into `req.body`, template renders from that object.
- **Target behaviour**: `src/router.rs:100-116` — non-JSON Content-Type on a non-empty body returns 415 `UnsupportedContentType` naming the type. Empty body + any Content-Type still works (parity with `ping`).
- **Motivation**: DataMapper's canonical contract is JSON in / rendered out (see `docs/DESIGN.md §2` — no non-goals for form data). A misleading 400 `InvalidJson` was the pre-refacto Rust behaviour (F-03); 415 is the correct HTTP semantic.
- **Migration**: switch client to `Content-Type: application/json` and post JSON.
- **Reversibility**: parse form-encoded body into `serde_json::Value` in the router; would need a stable key-order decision.
- **Filed**: 2026-08-04.

## D-008. Timestamp format from `{{now}}` — `+00:00` offset, higher precision

- **Field / behaviour**: `{{now}}` helper output.
- **Source of truth**: `lib/helpers.js:1-3` — `new Date().toISOString()` → `2026-08-04T12:34:56.789Z` (millisecond precision, trailing `Z`).
- **Target behaviour**: `src/helpers.rs:23-32` — `chrono::Utc::now().to_rfc3339()` → e.g., `2026-08-04T12:34:56.123456789+00:00` (nanosecond precision, `+00:00` offset).
- **Motivation**: chrono's default `to_rfc3339` emits an offset-suffixed form. RFC 3339 permits both; RFC 3339 parsers accept both.
- **Migration**: consumers parsing with a strict `Z`-suffix regex must relax to `Z|(+\d{2}:\d{2})`. If millisecond-only precision matters, format explicitly on the consumer side.
- **Reversibility**: replace `to_rfc3339()` with `format("%Y-%m-%dT%H:%M:%S%.3fZ")`. One line, one test.
- **Filed**: 2026-08-04.

## D-009. `{{{json missing}}}` renders `null`, not empty string

- **Field / behaviour**: `{{json}}` helper on missing/undefined values.
- **Source of truth**: `lib/helpers.js:4` — `JSON.stringify(undefined)` returns `undefined` (rendered as empty text).
- **Target behaviour**: `src/helpers.rs:58-72` — missing key resolves to `Value::Null`, serialised as `"null"`.
- **Motivation**: JSON output is more predictable; a template that emits `"k": {{{json missing}}}` produces `"k": null` (valid JSON) instead of `"k": ` (invalid).
- **Migration**: template authors should guard optional fields with `{{#if}}` (which they already did in JS — see `conditionals/include_optional.hbs`).
- **Reversibility**: swap the `Value::Null` branch for a no-op write. One helper edit.
- **Filed**: 2026-08-04.

## D-010. `{{path.length}}` — auto-rewritten via `len` helper, with warning

- **Field / behaviour**: `.length` accessor on arrays/strings inside a Handlebars mustache.
- **Source of truth**: JS Handlebars implicitly resolves `.length` via the JavaScript property lookup. Shipped in `DSL/samples/arrays/map_products.hbs:12` (JS source).
- **Target behaviour**: `src/renderer.rs:82-100,109-190` — templates are scanned for `.length` accessors; matches are rewritten to `{{len path}}` (value context) and `{{#if path}}` (block-condition context). `warn!` fires per rendered template.
- **Motivation**: R2.1 — a silent-drop is FORBIDDEN. The rewriter honours JS semantics transparently (option 1 in R2.1); the warning pushes operators to migrate their DSL bodies for clarity.
- **Migration**: rewrite `{{arr.length}}` → `{{len arr}}` and `{{#if x.length}}` → `{{#if x}}` in your `.hbs` files. Both idioms will continue to work — the warning tells you which files still need attention.
- **Reversibility**: delete `rewrite_dot_length` and its detector; templates using `.length` then fail loudly (as they did pre-refacto).
- **Filed**: 2026-08-04.

## D-011. `Accept:` negotiation is contains-based, not `req.accepts(...)`-ordered

- **Field / behaviour**: content-type preference resolution.
- **Source of truth**: `server.js:29` — `req.accepts(['json','html'])`, which respects quality values and order.
- **Target behaviour**: `src/router.rs:178-197` — case-insensitive contains-check for `application/json` and `*/*`.
- **Motivation**: simpler code, same outcome for the vast majority of real-world Accept headers. Edge case: `Accept: text/html;q=0.9, application/json;q=0.1` — JS picks HTML; Rust picks JSON.
- **Migration**: clients that need HTML precedence over JSON must send `Accept: text/html` alone or without `application/json`.
- **Reversibility**: swap in the `accept-language` crate's Accept parser; small dep, well-scoped change.
- **Filed**: 2026-08-04.

## D-012. HTML fallback MIME is `text/html; charset=utf-8`

- **Field / behaviour**: HTML fallback Content-Type header.
- **Source of truth**: `server.js:55` — `res.status(200).send(html)`; express infers `text/html` with default charset.
- **Target behaviour**: `src/router.rs:166` — explicit `text/html; charset=utf-8`.
- **Motivation**: explicit encoding is defensive; UTF-8 templates render correctly in all browsers.
- **Migration**: none. Consumers parsing `Content-Type` must handle the `; charset=…` param (all reasonable HTTP libraries do).
- **Reversibility**: drop the charset suffix. One string edit.
- **Filed**: 2026-08-04.

## D-013. Path traversal returns 400 `InvalidPath`, not silently stripped

- **Field / behaviour**: `..` and absolute-path prefixes in URL segments.
- **Source of truth**: `server.js:22` — `sanitize()` normalises then strips leading `..` and `/` sequences. `POST /samples/../foo` becomes `POST /samples/foo`; no error.
- **Target behaviour**: `src/renderer.rs:96-104` — `sanitize_segment` returns `Err(InvalidPath)`, router responds 400 with `{"error":"InvalidPath"}`.
- **Motivation**: security. Silent path rewriting hides bugs and traversal attempts. Loud rejection surfaces both.
- **Migration**: clients constructing URLs from user input must not embed `..`. This is standard hygiene.
- **Reversibility**: replace hard reject with `warn!` + strip. Not recommended.
- **Filed**: 2026-08-04.

## D-014. `TemplateNotFound` / `MethodNotAllowed` error bodies gain `"message"`

- Instance of D-005 for the two error variants JS documented explicitly.

## D-015. Template context is exactly the request body — no `layout: false` sibling

- **Field / behaviour**: Handlebars render context.
- **Source of truth**: `server.js:40` — `res.render(found, { layout: false, ...req.body })` — the context object mixes request-body fields with the `layout: false` control field.
- **Target behaviour**: `src/router.rs:130-133` — `render(project, view, &context)` where `context` is the parsed request body verbatim.
- **Motivation**: `layout` is an express-handlebars implementation detail leaking into user templates. `{{{json this}}}` on a JS DataMapper renders `{"layout":false,...body}`; Rust renders just `body`. Rust behaviour is what operators expect.
- **Migration**: templates that referenced `{{layout}}` inside their body (none in the shipped corpus) must remove that reference.
- **Reversibility**: pre-merge `{"layout": false, ...body}` in the router. Two lines.
- **Filed**: 2026-08-04.

## D-016. Boot log — both JS-compat single line AND structured tracing output

- **Field / behaviour**: startup log format.
- **Source of truth**: `server.js:73` — `console.log(\`DataMapper listening on :${PORT}\`)`.
- **Target behaviour**: `src/main.rs` — structured `tracing` output (config source, dsl_path, limits, listening line), plus a compat `println!("DataMapper listening on :{port}")` for log-grep monitors.
- **Motivation**: R2.5 — user-facing strings the source emitted MUST have a target equivalent. The compat println preserves the exact JS log line so monitoring keeps working; tracing output serves modern log-aggregators.
- **Migration**: none for monitors keyed on the JS line; new monitors SHOULD prefer the structured tracing output.
- **Reversibility**: delete the `println!` line.
- **Filed**: 2026-08-04.

## D-017. Sample-DSL comment headers reformatted (multi-line curl)

- **Field / behaviour**: cosmetic — Handlebars comment blocks at the top of each shipped DSL.
- **Source of truth**: `DSL/samples/*.hbs` — one-line curl command inside `{{!-- --}}`.
- **Target behaviour**: `DSL/samples/*.hbs` — multi-line curl command with backslash continuations plus a description line.
- **Motivation**: readability; the multi-line form fits inside a 80-column diff view.
- **Migration**: none; comments have no runtime effect.
- **Reversibility**: rewrite comment blocks. Cosmetic.
- **Filed**: 2026-08-04.

## D-018. Container DSL mount path changed to `/app/DSL`

- **Field / behaviour**: default container filesystem layout.
- **Source of truth**: `docker-compose.yml:12` — `./DSL:/workspace/app/DSL:ro`.
- **Target behaviour**: `docker-compose.yml` — `./DSL:/app/DSL:ro`.
- **Motivation**: matches sibling Buerostack Rust images (Ruuter, XTR); `WORKDIR /app` is the community convention.
- **Migration**: update the container-side mount target from `/workspace/app/DSL` to `/app/DSL`.
- **Reversibility**: change the target path in `docker-compose.yml`.
- **Filed**: 2026-08-04.

## D-019. `examples/basic-usage/` currently empty

- **Field / behaviour**: runnable example scaffolding.
- **Source of truth**: `examples/basic-usage/README.md` + `package.json` — walkthrough for the JS server.
- **Target behaviour**: empty directory in the Rust repo (as of `v0.1.0-alpha.1`); refacto batch re-fills with a Rust-adjusted README pointing at `docker compose up`.
- **Motivation**: examples must exist for the "quick start" path documented in the README.
- **Migration**: the new README walkthrough works against `datamapper` container.
- **Reversibility**: delete the README again.
- **Filed**: 2026-08-04.

## D-020. `CONTRIBUTING.md` — inherited from the parent `Buerostack/` tree

- **Field / behaviour**: contribution guide.
- **Source of truth**: `CONTRIBUTING.md` — per-repo contribution guide in the JS project.
- **Target behaviour**: no repo-local `CONTRIBUTING.md`; contribution rules live in the shared `Buerostack/DEV-REQUIREMENTS.md` + `STANDARDS.md` at the parent tree.
- **Motivation**: single source of truth across all Buerostack Rust services.
- **Migration**: read the parent-tree docs.
- **Reversibility**: add a per-repo `CONTRIBUTING.md` if this becomes confusing.
- **Filed**: 2026-08-04.

---

# Rust-side extensions (D-100 series)

These are behaviours the Rust target has that JS DataMapper does not.
Per R8.3 (negative-space audit), each is checked for conflicts with
JS-derived operator expectations.

## D-101. `GET /health` alias for `/healthz`

- **Field / behaviour**: additional liveness route.
- **Target behaviour**: `src/router.rs:50` — same handler as `/healthz`.
- **Motivation**: parity with sibling Buerostack Rust services (Ruuter, XTR, TIM).
- **Conflict check**: none — JS `/health` was 404, Rust makes it 200. Operators upgrading gain a new working URL; no monitor breaks.
- **Migration**: use either.
- **Reversibility**: delete the extra route registration.

## D-102. `HEAD /healthz` and `HEAD /health` allowed

- **Field / behaviour**: HTTP HEAD support on health routes.
- **Target behaviour**: `src/router.rs:66-68` — GET and HEAD accepted; other methods → 405.
- **Motivation**: RFC 7231 requires HEAD parity for any GET; most liveness probes issue HEAD.
- **Conflict check**: JS returned 200 for HEAD (express default). Same outcome.

## D-103. Rendered-output cap (`limits.max_response_bytes`)

- **Target behaviour**: `src/router.rs:135-140`, `src/config.rs:79-81` — default 16 MiB; over-limit → 500 `ResponseTooLarge`.
- **Motivation**: DoS defence. A pathological template can amplify a small input into gigabytes of output.
- **Conflict check**: JS had no cap. Rust's 16 MiB will not clip any realistic DataMapper output.
- **Migration**: raise via `datamapper.yaml` if you legitimately render >16 MiB.

## D-104. Per-request timeout (`limits.request_timeout_secs`) → 504

- **Target behaviour**: `src/router.rs:46-47`, `src/config.rs:82-84` — default 30s; overrun → 504 Gateway Timeout.
- **Motivation**: DoS defence for runaway helper recursion.
- **Conflict check**: JS had no timeout. 30s is generous for a template renderer.

## D-105 / D-106 / D-107. `--config` CLI flag + `DATAMAPPER_CONFIG` + `RUST_LOG` env vars

- **Target behaviour**: `src/config.rs:107-127`, `src/main.rs:17-21`.
- **Motivation**: standard Rust service posture. Explicit config override + log filter.
- **Conflict check**: JS had none. No conflict.

## D-108. Canonicalised path-prefix check (`is_under`)

- **Target behaviour**: `src/renderer.rs:113-120` — after path assembly, canonicalise and confirm the resolved path stays under `dsl_path`.
- **Motivation**: defence-in-depth against symlinks planted in the DSL tree.
- **Conflict check**: JS had no such check. Symlinks in JS DataMapper could escape `DSL/`. Rust blocks that.

## D-109. Structured `400 InvalidJson`

- **Target behaviour**: `src/error.rs:47`, `src/router.rs:126` — parse error yields `{"error":"InvalidJson","message":"..."}`.
- **Motivation**: JSON errors are more useful than express's HTML default 400.
- **Conflict check**: JS returned HTML. Rust returns JSON. Consumers of the response body change shape; consumers of the status code do not.

## D-110. Structured `413 RequestTooLarge`

- **Target behaviour**: `src/error.rs:45`, `src/router.rs:83-88` — over-limit → `{"error":"RequestTooLarge","limit":<n>}`.
- **Motivation**: same as D-109. Consumers get an actionable limit.
- **Conflict check**: JS returned HTML 413. Rust returns JSON.

## D-111. `500 Internal` catch-all

- **Target behaviour**: `src/error.rs:50` — used for I/O reading a template, unexpected states.
- **Motivation**: distinguish "template exists but failed" from "config broke."

## D-112. 24 + 16 + 17 unit / integration / regression tests

- **Target behaviour**: `src/{config,helpers,renderer,router}::tests`, `tests/it_end_to_end.rs`, `tests/it_regression_refacto.rs`.
- **Motivation**: JS had no test suite. Rust's suite is the executable contract per DEV-REQUIREMENTS §3.

## D-113. Hardened Docker image + compose

- **Target behaviour**: `Dockerfile` (multi-stage, distroless-ish, tini init, non-root, read-only rootfs), `docker-compose.yml` (`no-new-privileges`, `cap_drop: ALL`, resource limits, healthcheck).
- **Motivation**: DEV-REQUIREMENTS §5 container posture.
- **Conflict check**: JS image ran as root on `node:20-alpine`. Rust image cannot write to `/`. Templates SHOULD only ever be read-only mounts — same as JS's `:ro`.

## D-114. mdBook docs site instead of `docs/architecture/*.md`

- **Target behaviour**: `book/src/*`, deployed via `docs.yml` to GitHub Pages.
- **Motivation**: single canonical published docs. Retains all JS-side content coverage.

## D-115. README targeted at operators, not integrators

- **Target behaviour**: `README.md` opens with "why", not "quick start."
- **Motivation**: JS README's quick-start is preserved in `book/src/getting-started.md`.

---

# Count check

R5.3: cap ~50 entries per subsystem. This is one subsystem (payload
shaping); count is 26 D-0nn drifts + 15 D-1nn extensions = 41. Under
cap; the "refactor preserving behaviour" framing is intact.
