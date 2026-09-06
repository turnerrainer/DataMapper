# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/turnerrainer/datamapper/compare/v0.1.0-alpha.2...HEAD
[0.1.0-alpha.2]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.0-alpha.1
