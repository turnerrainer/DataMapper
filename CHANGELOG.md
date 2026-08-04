# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — REFACTO-REQUIREMENTS v1.0 compliance pass

Full compliance work against `Buerostack/REFACTO-REQUIREMENTS.md v1.0`
using JS `Buerostack/DataMapper` `v1.0.0` (`2025-09-24`) as the
source of truth.

- `docs/REFACTO-MATRIX.md` — §1.1 coverage matrix (55 rows: HTTP
  routes, config fields, CLI, env vars, helpers, content
  negotiation, path sanitisation, error bodies, template context,
  boot log, samples, tests, packaging, docs). Every row carries a
  planned status, a current status, and a verification level.
- `docs/REFACTO-PORT-PLAN.md` — §1.3 test-corpus port plan.
- `docs/REFACTO-AUDIT-S2.md` — §2 audit: 11 non-trivial findings
  across silent drops, unknown-field tolerance, default drift, and
  error-string parity.
- `docs/REFACTO-AUDIT-NEGATIVE-SPACE.md` — §8.3 negative-space
  audit: 21 Rust-side extensions checked against JS-derived
  operator expectations.
- `DIVERGENCES.md` — §5 divergence log with 26 documented drifts
  (D-0nn) + 15 documented extensions (D-1nn). Every entry names
  source and target locations, motivation, migration, reversibility.
- `MIGRATION.md` — §7.2 operator porting guide indexed to
  `DIVERGENCES.md`. Includes an operator checklist.
- `REFACTO-DEVIATIONS.md` — §10.2 record of MUSTs intentionally
  not met in this pass (default-drift documentation, timestamp
  format, cross-impl coverage scope).
- `compat/js-DSL/` — verbatim copy of the JS source-of-truth
  template corpus (§7.3).
- `compat/js-server/` — staged JS server for cross-impl repro.
- `tests/it_compat_js_dsl_corpus.rs` — §7.3 CI gate: every JS
  source-of-truth template renders end-to-end on Rust.
- `tests/it_repro_cross_impl.rs` — §4.3 cross-implementation
  fixtures: identical requests posted to both JS and Rust,
  responses diffed for equivalence modulo the documented
  divergence keys.
- `tests/it_regression_refacto.rs` — §4.4 regression tests, 17
  fixtures. Each guards one §2 finding and was written to fail
  before its fix.

### Changed — R2 fixes landed

- **F-01 / D-001**: Rust now reads the `PORT` env var as a fallback
  when the loaded config does not explicitly set `port:`. Fixes a
  silent config drop for operators porting JS deployments.
- **F-02 / D-010**: Templates using the JS `.length` accessor are
  auto-rewritten at load time — `{{arr.length}}` → `{{len arr}}`
  and `{{#if arr.length}}` → `{{#if arr}}`. A `warn!` fires per
  affected template. JS DSLs now port zero-touch.
- **F-03 / D-007**: Non-JSON request bodies return
  `415 UnsupportedContentType` naming the offending Content-Type,
  instead of a misleading `400 InvalidJson`.
- **F-04 / D-003**: Boot emits a `warn!` when `.hbs` files exist
  under `./views/` (the JS-side legacy root Rust no longer serves
  from).
- **F-05 / R6.1**: `#[serde(deny_unknown_fields)]` on `AppConfig`
  and `Limits` — a typo'd YAML field now hard-fails at parse.
- **F-11 / D-016**: The JS-compat boot line
  `DataMapper listening on :<port>` is emitted on stdout alongside
  the structured tracing output. Log-grep monitors keyed on the JS
  line continue to work.

### Added — new error variant

- `UnsupportedContentType(String)` on `DataMapperError`, mapped to
  HTTP 415 with the offending media type embedded in the message.

### Test coverage

- Baseline: 40 tests (24 unit + 16 e2e), all green.
- Post-refacto: 58 tests (24 unit + 16 e2e + 17 regression + 1
  compat corpus, plus 1 cross-impl repro when JS server is
  staged), all green.

### Known gaps (per §9.2)

- Timestamp format from `{{now}}` still uses `+00:00` and
  nanosecond precision (JS uses `Z` and millisecond). See D-008 /
  `REFACTO-DEVIATIONS.md`.
- Default `max_request_bytes` still 2 MiB binary (JS was 2 000 000
  decimal). See D-002 / `REFACTO-DEVIATIONS.md`.
- Cross-impl repro test is gated on a staged `compat/js-server/`
  with `node_modules/` populated; CI must set that up or set
  `DATAMAPPER_REPRO_STRICT=1` to gate on it.

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

[Unreleased]: https://github.com/turnerrainer/datamapper/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/turnerrainer/datamapper/releases/tag/v0.1.0-alpha.1
