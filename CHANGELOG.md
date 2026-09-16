# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0-alpha] - 2026-09-16

Security-hardening + observability release. Closes every h2ck.me
v1 audit-plus-break-test finding surfaced after `v0.1.3-alpha`
(N1, N2, N3, N5, N6, N7, N8, F-DM-1, FN-LOG-1..4) and adopts five
`FLEET-STRONGHOLDS.md` patterns (§1.6 W3C traceparent, §5.1
security response headers, §8.2 doctor CLI, §11.2 env-safety
posture gate, §11.3 dev-fixture DSL gate). One reflected-XSS
lane closed (N1 composite triple-brace + Accept: text/html),
plus four DoS-mitigation caps.

Delivered as a single rollup — see merged PR #18.

### Breaking

Every item below is client- or operator-observable. Details and
regression-test locations live in
[`CLAUDE.md`](./CLAUDE.md#rollup-deltas-pending--not-yet-in-a-tagged-release)
"Rollup deltas" and the
[Failure modes](./book/src/failure-modes.md) reference.

- **New response status codes on the error path** —
  `408 RequestReadTimeout` (slow-body/slow-loris, was silently
  folded into 504), `413 RequestArrayTooLarge` (request-body
  array over `limits.max_body_array_length`), `431
  HeaderValueTooLarge` (single request header > 8 KiB). Structured
  JSON bodies with `error`/`message` and finding-specific fields
  (`deadline_secs`, `length`+`limit`, `actual`+`limit`).
- **Global 404 shape changed** — path-encoded-traversal requests
  that used to hit Axum's empty-body default 404 now return the
  same `{"error":"NotFound","message":"...","tried":[]}` JSON as
  `TemplateNotFound`. Callers parsing the old empty body break.
- **`TemplateNotFound.tried` entries clipped at 256 chars each** —
  killed a 4× URL-length amplification vector. Callers displaying
  the paths verbatim see truncated strings on ultra-long routes.
- **Triple-brace templates (`{{{X}}}`) force `text/plain`** even
  when the client sends `Accept: text/html` — closes the composite
  XSS lane (attacker `Accept` + author's un-escaped output). The
  `{{{json obj}}}` helper is unaffected — its output is valid
  JSON and continues to serve as `application/json`.
- **`Accept:` header parsed per RFC 7231** — `;q=` quality values
  are honoured. `Accept: text/html;q=0` correctly excludes HTML;
  `Accept: application/json;q=0, text/html` correctly selects
  HTML. RFC-compliant callers already work; buggy `q=0` callers
  now go the right way.
- **Five default security response headers on every response** —
  `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`,
  `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload`,
  `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`,
  `Referrer-Policy: no-referrer`. Insert-if-absent semantics
  preserve any handler-set value. Callers that were parsing the
  bare 200-OK header set gain five new keys.
- **W3C `traceparent` + `x-trace-id` on every response** — inbound
  `traceparent` inherited when well-formed (version=00, 32-hex
  trace-id ≠ all-zeros); fresh `uuid::v4` otherwise. Span id is
  always regenerated (this service's span, not the caller's).
- **Env-safety refusal-to-boot in non-dev** — with `APP_ENV`
  (or `ENVIRONMENT` / `DEPLOY_ENV`) set to anything other than
  `dev` (unknown values fail-safe to `Production`), boot aborts
  when the DSL root is writable OR any `.hbs` under `dsl_path`
  matches a dev-fixture pattern (`dev-login`, `mock-`, `-mock`,
  `/test/`, `example-`, `-example`, `/dev/`, `/mocks/`). Fix the
  posture or set `APP_ENV=dev` for local runs.
- **CLI surface grew subcommands** — `datamapper serve` (default,
  bare invocation still resolves) and `datamapper doctor
  [--strict]`. `--config <path>` moves to the global argument
  slot. Scripts that pass positional args other than a subcommand
  break; scripts calling `datamapper` bare still work.
- **`limits.request_timeout_secs` is now wired end-to-end** — was
  advisory-only on the `TimeoutLayer` at a hard-coded 30 s. The
  same value now also bounds body read (see the new 408). Custom
  values in `datamapper.yaml` take effect on both caps.

### Security

- **N1 composite XSS** — reflected-XSS lane closes for templates
  that use `{{{userInput}}}` and callers that send `Accept:
  text/html`. Boot WARN per offending template.
- **N2 M2 bypass** — `Accept: */*` still does NOT enable HTML
  fallback (unchanged from M2). `q=0` exclusion is now honoured.
- **N3 slow-loris** — body read deadline separates slow-body
  (408) from render/response-write timeout (504). Prevents
  request-slot starvation via slow-drip uploads.
- **N5 URL-length amplification** — `tried:` paths clipped at
  256 chars each.
- **N6 404 shape unification** — path-encoded-traversal cannot
  differentiate route-not-registered from template-not-found via
  response-shape probe.
- **N7 config parse error redaction** — misconfigured `--config`
  pointing at a mounted secret file no longer leaks contents to
  log aggregators. Location (line, column) preserved; value
  elided.
- **N8 header amplification** — application-layer per-header
  value cap at 8 KiB (matches nginx / Cloudflare defaults; covers
  legit JWTs / session cookies / W3C traceparent).
- **F-DM-1 render amplification** — request-body array length
  bounded before render (default 10 000); `{{#each}}` DoS caps
  at cost. `CappedWriter` (from M3) enforces the response cap
  mid-render.
- **FLEET §11.2** — writable DSL root upgrades from WARN
  (v0.1.3-alpha) to REFUSE-TO-BOOT in non-dev.
- **FLEET §11.3** — dev-fixture DSL refusal in non-dev.

### Added

- `datamapper doctor [--strict]` subcommand — pre-boot validator
  with no side effects. Grep-friendly `[ok]/[warn]/[error]`
  output. Exit 0 on clean, 1 on error (or on warn with
  `--strict`). See `book/src/configuration.md` §2.1.
- `limits.max_body_array_length` config field (default `10000`,
  `0` disables). See `book/src/configuration.md` §3.
- `access_log` middleware — one INFO line per request:
  `http_request_completed method=X route=Y status=Z
  duration_us=... trace_id=...`. Matched route pattern only; no
  headers, bodies, client IPs, or raw URIs.
- `security_headers`, `traceparent`, `env_safety`, `access_log`,
  `doctor` modules in `src/`.

### Changed

- Tracing subscriber emits plain-text (no ANSI escapes) when
  stderr is not a TTY — `docker logs` / `journalctl` capture
  SIEM-clean bytes (FN-LOG-1/2).
- Config parse errors go through `redact_serde_error` — invalid
  YAML values elided, location preserved.
- Router extracts `Request` (not `Bytes`) so the body read is
  wrapped in an explicit `tokio::time::timeout`.

### Dependencies

- **New**: `uuid = "1"` (default-features off, `v4` only) — for
  W3C `traceparent` trace-id generation.
- **New**: `clap = "4"` (`derive` feature) — for the
  `datamapper doctor` subcommand.
- **Bumped**: `rustls 0.23.43 → 0.23.45` (transitive via
  dev-only `reqwest`) — clears **RUSTSEC-2026-0285** (medium,
  5.3: TLS 1.3 handshake messages incorrectly accepted across
  encryption level boundaries). No runtime effect on the
  DataMapper binary — `reqwest` is a `[dev-dependencies]` entry
  used by the integration-test harness only.

### Test coverage

- **155 tests** total, up from 66 on `v0.1.3-alpha` (+89 new).
  96 lib unit + 40 end-to-end + 17 regression-refacto + 1 JS
  compat corpus + 1 cross-impl repro.
- `cargo audit --deny warnings`, `cargo deny check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`, and `mdbook build` (with linkcheck) all clean.

### Upgrade notes

Detailed upgrade guidance for an operator moving from
`v0.1.3-alpha` lives in
[`CLAUDE.md` §"Upgrading from v0.1.3-alpha"](./CLAUDE.md#upgrading-from-v013-alpha-to-v020-alpha).
Short version:

- If your reverse proxy overrides any of the 5 security headers,
  nothing changes — the middleware is insert-if-absent.
- If your CI parses the bare 404 response body, update the
  parser to accept the structured JSON shape (same shape as
  `TemplateNotFound`).
- If your deployment runs bare `datamapper` with positional args,
  switch to `datamapper serve` explicitly.
- If your deployment runs a writable DSL mount in production,
  either fix the mount (`:ro`) or set `APP_ENV=dev`.

## [0.1.3-alpha] - 2026-09-06

Security-hardening release addressing the h2ck.me v1 pre-publication
audit (all 5 findings closed, ✅ pass verdict) plus a Dockerfile CVE
sweep and CI-actions modernisation. No user-facing feature changes;
DSL and HTTP surface are byte-identical to alpha.2.

### Security

- **M1 — HTML-safety guidance in templating docs**: new
  `book/src/handlebars-helpers.md` section spelling out `{{expr}}`
  (escaped, default) vs `{{{expr}}}` (raw) with an escape-behaviour
  table, three bad/good pairs, and a rule for when raw is safe.
- **M2 — `Accept`-negotiation is explicit-opt-in for HTML**: the
  fallback response `Content-Type` defaults to `text/plain` unless
  the request `Accept` header explicitly names `text/html`. `*/*`
  does **not** count as HTML opt-in. Fixes a
  reflected-content-type vector on operator-hosted error pages.
  Pinned by six regression tests.
- **M3 — Response cap enforced mid-render**: new `CappedWriter` in
  `src/renderer.rs` short-circuits Handlebars rendering the instant
  the byte budget is exceeded, replacing the render-then-check path
  that allowed transient GiB-scale allocations under an amplification
  template. Pinned by a 32×32×1024-byte amplification test against
  a 10 KiB cap.
- **I1 — `cors_origin` refuses to boot**: setting the deprecated
  `cors_origin` field in config now aborts startup with a diagnostic
  naming the file:line and pointing at reverse-proxy CORS. Was a
  silent no-op that could mislead operators.
- **L1 — Boot-time DSL-writability probe**: `src/main.rs` performs
  a `create_new` probe under `dsl_path` at startup and emits a
  `WARN` if writable, catching bind-mount / read-only quirks that
  `stat` alone misses. `SECURITY.md` updated with the hardening
  rationale.

### Changed

- **Dockerfile base-image CVE sweep** (Snyk-driven, PR #2) —
  reduces HIGH/CRITICAL findings so the publish workflow's Trivy
  gate stays green.
- **CI actions bumped to Node-24 majors** (`actions/checkout@v5`,
  `actions/cache@v5`, `actions/upload-pages-artifact@v5`,
  `actions/deploy-pages@v5`) — silences the runner's Node-20
  deprecation warning and drops the compat-shim dependency.
- **`chacha20` bumped 0.10.1 → 0.10.2** (0.10.1 was yanked).

### Test coverage

- 66 tests total (up from 59 in alpha.2): the seven added tests
  pin the M2 explicit-opt-in behaviour and the M3 mid-render cap.
- `cargo audit`, `cargo deny`, `cargo clippy -D warnings`, and
  `cargo fmt --check` all clean.

## [0.1.0-alpha.2] - 2026-08-05

### Added — JS-source-of-truth compatibility

Compatibility work against the JS `Buerostack/DataMapper` `v1.0.0`
(`2025-09-24`) source of truth so existing JS deployments and DSLs
port zero-touch. Full porting summary in
[Porting from JS DataMapper](../porting-from-js.md) on the docs site.

- **`PORT` env var honoured as fallback** when the loaded config
  does not explicitly set `port:`. Fixes a silent config drop for
  operators lifting JS docker-compose / systemd unit files.
- **Auto-rewrite of the JS `.length` accessor**: `{{arr.length}}` is
  rewritten to `{{len arr}}` and `{{#if arr.length}}` to
  `{{#if arr}}` at load time, with a `warn!` per affected template.
  Ported JS DSLs render correctly without hand-editing.
- **`415 UnsupportedContentType`** for non-JSON request bodies,
  naming the offending Content-Type in the JSON error body. Replaces
  a misleading `400 InvalidJson` for form-encoded posts.
- **Boot `warn!`** if `.hbs` files still live under `./views/` — the
  JS-side legacy root Rust no longer serves from — so operators get
  a single boot-log signal instead of silent 404s.
- **Boot INFO** aggregating templates that still use the JS `.length`
  accessor, so operators can prioritise migration work at a glance.
- **JS-compat single-line boot log** —
  `DataMapper listening on :<port>` — emitted on stdout alongside
  the structured tracing output. Log-grep monitors carried over from
  the JS deployment keep working.

### Changed

- **`#[serde(deny_unknown_fields)]`** on `AppConfig` and `Limits`
  structs — a typo'd YAML field now hard-fails at parse instead of
  silently no-op'ing.
- **`UnsupportedContentType(String)`** added to `DataMapperError`,
  mapped to HTTP 415.

### Docs

- New docs page: JS→Rust porting summary.
- New docs page: walkthrough of every sample DSL with the exact
  curl command and expected response.
- New docs page: dedicated Handlebars-helper reference with
  runnable examples, including migration notes.
- Expanded configuration reference to cover `PORT` env var,
  auto-rewrite behaviour, and every boot-log line an operator will
  see.
- Failure-modes reference gains `UnsupportedContentType` (415) and
  `RequestTimeout` (504) rows.

### Test coverage

- Baseline: 40 tests (24 unit + 16 e2e).
- Post-alpha.2: 59 tests (24 unit + 16 e2e + 17 regression + 1
  compat corpus + 1 cross-impl repro).

### Known gaps

- Timestamp format from `{{now}}` still uses `+00:00` and
  nanosecond precision (JS uses `Z` and millisecond).
- Default `max_request_bytes` still 2 MiB binary (JS was 2 000 000
  decimal).
- Cross-impl repro test needs a staged `compat/js-server/` — see
  `scripts/setup-repro.sh`.

## [0.1.0-alpha.1] - 2026-07-29

### Added
- Initial Rust re-implementation of DataMapper, targeting DEV-REQUIREMENTS
  compliance from day one.
- `POST /:project/*view` — Handlebars template folder-drop routing.
  Templates under `DSL/<project>/<view>.hbs` become HTTP endpoints.
- `GET /healthz` + `GET /health` — liveness probe (JSON body).
- Content negotiation: `type: json` request header + `Accept:` header
  precedence, opportunistic JSON MIME upgrade when the rendered output
  parses as JSON, `text/html` fallback for non-JSON output.
- Built-in Handlebars helpers: `{{now}}`, `{{{json obj}}}`,
  `{{len items}}`.
- YAML configuration (`datamapper.yaml`) with search-path resolution
  (`--config` CLI flag, `DATAMAPPER_CONFIG` env var, `./datamapper.yaml`,
  built-in defaults).
- Request/response size caps and per-request timeout guardrails.
- Two-layer path-traversal defence (lexical + canonicalised prefix
  check).
- Structured error responses mapped to appropriate HTTP status codes.
- 11 sample DSL templates under `DSL/samples/` covering
  arrays, conditionals, config-lookup, objects, strings, transforms,
  user CRUD, and nested-each patterns.
- Multi-stage Dockerfile: non-root user, `tini` init, read-only
  rootfs, self-contained image (config + samples baked in).
- Production-hardened `docker-compose.yml`: `no-new-privileges`,
  `cap_drop: ALL`, resource limits, healthcheck.
- Four GitHub Actions workflows: `tests` (amd64 + arm64 matrix + docs
  build), `security` (cargo-audit + cargo-deny, daily cron), `publish`
  (multi-arch build, Trivy scan gate, cosign keyless signing to Docker
  Hub + GHCR), `docs` (GitHub Pages deployment).
- mdBook documentation site: introduction, getting started,
  configuration reference, failure modes, changelog.
- Task tracking under `tasks/` — task 001 (domain deep-dive) and task
  002 (MVP rewrite) landed; backlog items 003 (OTel traceparent),
  004 (JSON-schema validation), 005 (helper expansion) filed.
- 24 unit + 16 integration tests, all green.

[Unreleased]: https://github.com/turnerrainer/datamapper/compare/v0.1.3-alpha...HEAD
[0.1.3-alpha]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.3-alpha
[0.1.0-alpha.2]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.0-alpha.1
