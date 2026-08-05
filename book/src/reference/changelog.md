# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.2] - 2026-08-05

### Added — JS-source-of-truth compatibility

Compatibility work against the JS `Buerostack/DataMapper` `v1.0.0`
source of truth so existing JS deployments and DSLs port zero-touch.
Full porting summary in [Porting from JS DataMapper](../porting-from-js.md).

- **`PORT` env var honoured as fallback** when the loaded config does
  not explicitly set `port:`.
- **Auto-rewrite of the JS `.length` accessor** at template-load time:
  `{{arr.length}}` → `{{len arr}}` and `{{#if arr.length}}` →
  `{{#if arr}}`. Warn per affected template. Ported JS DSLs render
  correctly without hand-editing.
- **`415 UnsupportedContentType`** for non-JSON bodies, naming the
  offending Content-Type.
- **Boot `warn!`** if `.hbs` files still live under `./views/`.
- **Boot INFO** listing templates still using the JS `.length`
  accessor.
- **JS-compat single-line boot log**
  (`DataMapper listening on :<port>`) alongside structured tracing.

### Changed

- `#[serde(deny_unknown_fields)]` on `AppConfig` and `Limits` — a
  typo'd YAML field now hard-fails at parse.
- `UnsupportedContentType(String)` added to `DataMapperError`, mapped
  to HTTP 415.

### Docs

- New page: [Porting from JS DataMapper](../porting-from-js.md).
- New page: [Handlebars helpers](../handlebars-helpers.md).
- New page: [Sample DSLs](../samples.md).
- Expanded [Configuration](../configuration.md) with boot-log
  diagnostics, `PORT` env var, and runnable config samples.
- Expanded [Failure modes](../failure-modes.md) with 415 + 504 rows.

### Test coverage

- 40 → 59 tests (24 unit + 16 e2e + 17 regression + 1 compat corpus
  + 1 cross-impl repro).

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
