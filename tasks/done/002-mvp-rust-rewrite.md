# 002 — MVP: Rust re-implementation of DataMapper

## Filed
2026-07-29 — scoping the initial cut per `docs/DESIGN.md`.

## Landed
2026-07-29 — commit `<sha>`. Ships:
- axum-based HTTP server (`src/main.rs`, `src/router.rs`)
- YAML config with search-path resolution (`src/config.rs`)
- Handlebars renderer with strict traversal guards (`src/renderer.rs`)
- Built-in `now`, `json`, `len` helpers (`src/helpers.rs`)
- Structured error type → HTTP status mapping (`src/error.rs`)
- 24 unit + 16 integration tests, all green

## Severity
High. Blocks every downstream task.

## Motivation
DEV-REQUIREMENTS.md mandates that all Buerostack "core-component-on-Rust"
projects meet a common bar for reliability, security, and
observability that the Node.js DataMapper does not. The rewrite
lands the same wire behaviour (existing DSLs migrate with a single
`{{foo.length}}` → `{{len foo}}` rename) on the compliant base.

## Fix / Design
- Preserve wire behaviour: `POST /:project/*view`, `GET /healthz`,
  `type: json` header + `Accept:` negotiation, template folder-drop
  under `dsl_path`, two-candidate lookup (`<view>.hbs`,
  `hbs/<view>.hbs`).
- Add: request/response size caps, sanitise + canonicalise path
  segments, structured error responses, non-strict Handlebars for
  DSL compatibility, `len` helper for the `.length` compat gap,
  `RUST_LOG`-controlled `tracing` logs.
- Cut anything the Node.js server had that's now handled at a
  different layer (there's nothing — DataMapper's surface is small).

## Acceptance
- [x] `POST /:project/*view` renders `.hbs` templates.
- [x] `GET /healthz` returns `{service, ok, ts}`.
- [x] `wants_json` mirrors Node.js DataMapper behaviour.
- [x] Size caps enforced with 413 (request) / 500 (response).
- [x] Path traversal blocked (`..`, `\0`, absolute) at two layers.
- [x] All 11 shipped sample DSLs render successfully in an
      integration test.
- [x] `cargo fmt --check` clean.
- [x] `cargo clippy --all-targets -- -D warnings` clean.
- [x] `cargo test --no-fail-fast` green.

## Estimated effort
1 day.

## Dependencies
- 001 (domain design)

## Non-scope
- OpenTelemetry (deferred to backlog).
- JSON-schema validation (deferred to backlog).
- Admin API — deliberately never (DEV-REQUIREMENTS §5.3).
- HTTP → HTTPS termination (owned by the ingress in front).

## Risks
- Handlebars compat surface — mitigation: every shipped sample DSL
  has an end-to-end test that hits it with the exact JSON body its
  header comment recommends. Guards regressions from crate bumps.
