# DataMapper — Domain design

**Written**: 2026-07-29
**Author**: Rainer Türner
**Status**: authoritative for v0.1.x

This document defines what DataMapper must do, why it exists,
and what is explicitly out of scope. Task 001 (`tasks/done/001-domain-deep-dive.md`)
tracks its landing.

---

## 1. Purpose

DataMapper is the **payload-shaping layer** of the Buerostack stack.
It sits between backend service providers and payload consumers
(typically UIs and downstream services) and does two jobs:

1. **Normalise and shape** heterogeneous provider payloads into
   ready-to-use response bodies. Consumers don't massage JSON on the
   client — the shape they need is what they get on the wire.
2. **Act as a contract layer** between backend services. A UI targets
   the DataMapper endpoint, not the upstream. When an upstream
   changes its wire shape, only the DataMapper template changes.

DataMapper is **architecturally important**, not application logic.
It doesn't implement business rules; it implements the *lack* of
per-consumer bespoke transformation code.

## 2. Non-goals

DataMapper deliberately does NOT:

- **Fetch data from upstreams.** No outbound HTTP, no database
  connections, no message queues. The request body IS the input.
  Composition of upstream calls belongs in Ruuter (which then
  passes the aggregated body to DataMapper).
- **Persist state.** DataMapper is stateless per request. No caching,
  no session, no disk writes at runtime. Templates are read-only
  inputs.
- **Validate business rules.** Schema validation, auth, rate
  limiting are all cross-cutting concerns owned by other layers
  (Ruuter, TIM). DataMapper trusts its input.
- **Support arbitrary code execution.** Handlebars is intentionally
  declarative and does not permit `eval`-shaped constructs. Adding
  helpers that shell out, read arbitrary files, or open sockets is
  out of scope.
- **Emit HTML for browsers.** HTML output is a *fallback* MIME for
  historical compatibility. The canonical output is JSON.

## 3. Data flow

```
    HTTP client
        │
        ▼
    POST /:project/*view    (JSON body)
        │
        │  sanitise segments, guard traversal
        ▼
    dsl_path/<project>/<view>.hbs
    (fallback: dsl_path/<project>/hbs/<view>.hbs)
        │
        │  read template file
        ▼
    Handlebars(reg + helpers).render(template, body)
        │
        │  wants_json(headers) ?
        │      yes → serialize as application/json
        │       no → try JSON parse; on success → JSON;
        │           on failure → text/html fallback
        ▼
    HTTP response
```

Everything on that path is synchronous and pure once the axum
handler runs. No async I/O other than reading the request body and
writing the response — the template file is read on every request
(no cache; see §6 for the rationale).

## 4. Route model

```
POST /<project>/<view>       — the workhorse. `<view>` may contain
                              `/`; the axum `*view` catch-all
                              captures the tail. `<project>` and
                              every segment of `<view>` are
                              sanitised individually.

GET  /healthz                — liveness probe.
GET  /health                 — alias (parity with Ruuter, XTR).
HEAD /healthz, HEAD /health  — allowed.

Any other method on /healthz or /health → 405 with JSON.
```

Template resolution is:
1. `<dsl_path>/<project>/<view>.hbs`
2. `<dsl_path>/<project>/hbs/<view>.hbs`

The second candidate exists solely to preserve the original
DataMapper Node.js convention of grouping template files under an
`hbs/` subfolder inside a project.

## 5. Output negotiation

Preserves the original Node.js DataMapper behaviour so existing
consumers migrate unchanged:

| Signal                                | Effect                                    |
|---------------------------------------|-------------------------------------------|
| `type: json` request header (any case) | JSON MIME. Parse-fallback to raw+JSON MIME. |
| `Accept:` contains `application/json` | JSON MIME.                                |
| `Accept:` contains `*/*` (no text/html) | JSON MIME.                              |
| No JSON preference, output parses as JSON | JSON MIME (opportunistic upgrade).     |
| No JSON preference, output does NOT parse as JSON | `text/html; charset=utf-8`.      |

The "opportunistic JSON when the output looks like JSON" behaviour
is deliberate — a template that renders `{ "ok": true }` is served
as JSON to any client that doesn't explicitly demand HTML.

## 6. Design decisions

### 6.1 Templates read per request, not cached

The Node.js DataMapper re-read from disk every request; operators
depend on this — edit the DSL, retry the curl, see the change. We
preserve that behaviour. The cost is a `read` syscall per request;
for a component that fronts hand-written templates in the low
hundreds this is not a bottleneck.

### 6.2 Non-strict Handlebars mode

The original Node.js DataMapper used JS Handlebars in its default
(non-strict) mode. Missing keys render as empty strings and
downstream `{{#if}}` / `{{#unless}}` guards do the routing.
`handlebars-rust`'s `set_strict_mode(true)` would break every
shipped sample template. Non-strict is the compat baseline. DSL
authors guard optional fields with `{{#if}}` at the template layer.

### 6.3 Path traversal defence

Two independent checks:
1. Lexical: `sanitize_segment` rejects `..`, absolute prefixes,
   null bytes, empty segments *before* any filesystem touch.
2. Structural: `is_under` canonicalises the assembled path and
   confirms it stays inside `dsl_path` — a defence against
   symlinks planted in the DSL tree.

### 6.4 No admin HTTP surface

Following DEV-REQUIREMENTS §5.3, DataMapper deliberately exposes
no admin, no reload, no metrics endpoints in-process. Operational
concerns live at the infra layer (SIGHUP a fresh container is the
reload; scrape logs via Loki, spans via Tempo). Baking auth into a
process that also serves consumer traffic conflates two attack
surfaces.

### 6.5 Response body cap

Templates can amplify — a small input can render a huge output.
`max_response_bytes` is the guardrail against runaway helper
recursion, accidental `{{{json bigThing}}}` calls, and
denial-of-service via memory pressure. Cap is post-render (already
in memory); a stream-oriented renderer could push this earlier
but adds complexity without a real-world driver.

### 6.6 Custom `len` helper

JS Handlebars resolves `foo.length` on arrays and strings via the
implicit `.length` property on JS objects. `handlebars-rust` has
no equivalent. Rather than teach every DSL author about that
gotcha, we ship a `{{len xs}}` helper that returns the length of
arrays, strings, or objects (0 for null/missing). Migrating a
Node.js DSL is a two-token find-and-replace: `{{xs.length}}` →
`{{len xs}}`.

## 7. Configuration

Full reference in `book/src/configuration.md`. The shipping
`datamapper.yaml` defines every knob; environment variables that
influence runtime behaviour are limited to `RUST_LOG` (tracing
filter) and `DATAMAPPER_CONFIG` (config file override path).

## 8. Failure modes

Full HTTP-status/error-code table in
`book/src/failure-modes.md`. Every variant in
`error::DataMapperError` maps to a documented status.

## 9. What "done" looks like for v0.1.0

- Every rule in DEV-REQUIREMENTS.md §§ 1–11 satisfied.
- Zero-cost migration path from the Node.js DataMapper: existing
  `.hbs` templates run unchanged after `{{foo.length}}` →
  `{{len foo}}` (the only known compatibility fracture).
- Container image published to Docker Hub + GHCR, multi-arch,
  cosign-signed.
- mdBook deployed to GitHub Pages.
- 40+ tests (unit + integration), all green in CI matrix
  (amd64 + arm64).

## 10. What v0.2.x might add

Speculative, not committed:

- OpenTelemetry `traceparent` propagation (PATTERNS.md §4).
- Optional JSON-schema validation of request bodies per DSL.
- Optional per-DSL rate limiting.

None of these are on the roadmap for v0.1.x — see `tasks/backlog/`
for the working list.
