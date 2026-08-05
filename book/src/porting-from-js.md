# Porting from JS DataMapper

If you're moving from the JS `Buerostack/DataMapper` (v1.0.0)
implementation, this is the one page you need. The Rust
implementation is a behaviour-preserving reimplementation with a
small, deliberate set of differences — every one of them called out
here, with the migration action for each.

The goal: a JS deployment (compose file, DSLs, monitors) can be
migrated with only the changes on this page.

---

## TL;DR — the operator's checklist

- [ ] Swap the container image name.
- [ ] Change the container-side DSL mount target from
      `/workspace/app/DSL` → `/app/DSL`.
- [ ] Keep `PORT` env var if you used it — still honoured.
- [ ] Existing `.hbs` templates work unchanged (`.length` is
      auto-rewritten).
- [ ] If any client posts `application/x-www-form-urlencoded`,
      switch to `application/json`.
- [ ] If any monitor asserts strict-equality on error JSON
      bodies, allow the extra `message` field.

---

## §1 Config

### Port

JS: `PORT` env var (default 3000).

Rust: primary knob is `port:` in `datamapper.yaml`. `PORT` env var
is honoured as a fallback iff the config file omits `port:`, so
`PORT=8181 datamapper` still works. See
[Configuration §2](./configuration.md#22-environment-variables).

**Migration:** keep-as-is if you rely on `PORT`. New deployments
should move the value into `datamapper.yaml`.

### Inbound body limit

JS default: `express.json({ limit: "2mb" })` → 2 000 000 bytes.

Rust default: `limits.max_request_bytes: 2097152` → 2 MiB (binary).

Delta is ~5%. No realistic client is threading that needle, but if
you specifically want the JS-exact 2 000 000-byte limit, set it
explicitly:

```yaml
limits:
  max_request_bytes: 2000000
```

### Response cap and request timeout (new)

Rust adds two guardrails JS did not have:

- `limits.max_response_bytes` (default 16 MiB) — rendered output
  cap, overflow → 500 `ResponseTooLarge`.
- `limits.request_timeout_secs` (default 30) — per-render wall
  clock, overrun → 504 Gateway Timeout.

Raise via `datamapper.yaml` if a legitimate template renders bigger
or slower than the defaults.

### `views/` and layouts

JS also served `.hbs` files from a `views/` directory alongside
`DSL/`, with express-handlebars `layoutsDir` support.

Rust searches only `dsl_path`. Layout blocks (`{{#extend "…"}}`)
are unsupported.

**Migration:** move `views/<project>/<view>.hbs` under `dsl_path/`.
Inline any layout content into the template body. DataMapper warns
at boot if it finds `.hbs` files parked under `./views/`.

---

## §2 DSL / template syntax

### `.length` accessor — auto-rewritten

The only DSL-syntax difference between the two implementations.

JS Handlebars resolves `{{arr.length}}` via the implicit
JavaScript `.length` property; `handlebars-rust` does not.
DataMapper detects `.length` usage at load time and rewrites:

| You write | DataMapper renders as if you wrote |
|---|---|
| `{{arr.length}}` | `{{len arr}}` |
| `{{#if arr.length}}...{{/if}}` | `{{#if arr}}...{{/if}}` |
| `{{#unless arr.length}}...{{/unless}}` | `{{#unless arr}}...{{/unless}}` |

A `warn!` log line names each affected template so you can update
the source files at your leisure. Semantics are identical either
way; the rewrite is purely for engine compatibility.

Silencing the warning means rewriting the templates by hand — see
[Handlebars helpers](./handlebars-helpers.md#length-accessor--auto-rewritten-for-compat).

### `{{{json this}}}` and template context

JS express-handlebars passes `{ layout: false, ...req.body }` as
the render context, so `{{{json this}}}` on the JS side may leak
express internals into the response (fields like `layout`,
`settings`, `_locals`, `cache`).

Rust passes the request body verbatim as the render context, so
`{{{json this}}}` returns exactly the request body — the leaky
JS internals are gone.

**Migration:** none for templates that never referenced the leaked
keys (the shipped corpus doesn't). If any of your templates
explicitly emitted `{{layout}}` or the other express fields,
remove those references.

### `{{now}}` timestamp shape

JS: `new Date().toISOString()` → `2026-08-05T12:34:56.789Z` (`Z`
suffix, millisecond precision).

Rust: `chrono::Utc::now().to_rfc3339()` →
`2026-08-05T12:34:56.123456789+00:00` (`+00:00` suffix, nanosecond
precision).

Both are valid RFC 3339. Any consumer parsing with a strict
`Z`-only regex must relax to `Z|(+\d{2}:\d{2})`.

### `{{{json missing}}}` and missing keys

JS: `JSON.stringify(undefined)` emits nothing.

Rust: missing key resolves to `null`, serialised as the string
`null`.

**Migration:** if a template does `"k": {{{json optional}}}` and
you relied on the field disappearing, guard with `{{#if}}`
(templates that used the JS behaviour already did this — see
`conditionals/include_optional.hbs`).

---

## §3 HTTP surface

### Routes

| Method + Path | JS | Rust | Migration |
|---|---|---|---|
| `POST /<project>/<view>` | works | works | none |
| `GET /healthz` | JSON body | JSON body (same shape) | none |
| `GET /health` | 404 | 200 (alias) | new working URL; safe |
| `HEAD /healthz`, `HEAD /health` | 200 | 200 | none |
| `POST /healthz` | 405 `{"error":"MethodNotAllowed"}` | 405 body adds `"message"` | monitors on `"error"` field unchanged |
| Non-POST on `/:project/*` | 404 HTML | 405 `allow: POST` | more informative; no action needed |
| Path traversal (`../etc/passwd`) | silent sanitise → 404 | 400 `InvalidPath` | fix clients that construct URLs with `..` |

### Request Content-Type

JS accepted `application/x-www-form-urlencoded` bodies via
`express.urlencoded`.

Rust returns 415 `UnsupportedContentType` for non-JSON bodies,
naming the offending Content-Type in the JSON error message.

**Migration:** switch such clients to `application/json`.
`application/vnd.api+json` and other `+json` variants continue to
work. Empty bodies with any Content-Type also continue to work
(parity with JS `ping`).

### Error response bodies

JS: `{"error":"<code>"}` plus per-variant extras.

Rust: `{"error":"<code>","message":"<human string>"}` plus the
same per-variant extras. The machine-facing `"error"` key is
unchanged; log-grep monitors on `"error":"TemplateNotFound"` etc.
still fire.

**Migration:** if a monitor asserts strict-equality on the error
body, allow the extra `"message"` field. Grep-style monitors need
no change.

New error codes (each replaces a JS unstructured response):

- `InvalidJson` (400) — was express default HTML 400
- `InvalidPath` (400) — was silent-strip on JS side
- `RequestTooLarge` (413) — was express default HTML 413
- `ResponseTooLarge` (500) — no JS equivalent (new cap)
- `Internal` (500) — no JS equivalent
- `UnsupportedContentType` (415) — was silently accepted as form data on JS

Full list: [Failure modes](./failure-modes.md).

---

## §4 Logging

### Boot line

Rust emits **both** the JS-compatible single line and the
structured tracing output:

```text
2026-08-05T12:00:00.001Z  INFO datamapper: listening on 0.0.0.0:3000
DataMapper listening on :3000
```

**Migration:** none. Log-grep monitors keyed on
`DataMapper listening on :<port>` continue to work.

### Structured tracing

`RUST_LOG` filter controls verbosity (`info` default, `debug` adds
per-request detail). Multi-line structured records replace the
JS `console.log` output. See
[Configuration §3](./configuration.md#3-boot-log-diagnostics).

---

## §5 Container migration

### Image

The Rust image is at:

- `docker.io/turnerrainer/datamapper:0.1.0-alpha.2`
- `ghcr.io/turnerrainer/datamapper:0.1.0-alpha.2`
- `docker.io/turnerrainer/datamapper:alpha` (floating pre-release)
- `ghcr.io/turnerrainer/datamapper:alpha`

Multi-arch (amd64 + arm64), Trivy-scanned, cosign-signed.

### Compose

```yaml
services:
  datamapper:
    image: docker.io/turnerrainer/datamapper:alpha
    ports:
      - "3000:3000"
    volumes:
      - ./DSL:/app/DSL:ro                    # NOTE mount target
      - ./datamapper.yaml:/app/datamapper.yaml:ro
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://localhost:3000/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
```

Key deltas from the JS compose:

- Image name changed.
- **Mount target** changed: `/workspace/app/DSL` → `/app/DSL`.
- Hardening flags added (`read_only`, `cap_drop`, `no-new-privileges`,
  tmpfs, resource limits).
- `datamapper.yaml` is a new mount point for runtime config.

### Rollback

If Rust behaviour is unacceptable in a specific slot, the JS image
still exists at whatever tag you were pinning pre-migration. To
roll back:

- Swap the image.
- Change the mount target back to `/workspace/app/DSL`.
- Keep the DSL tree — it's the same shape.

---

## §6 What did NOT change

Deliberate list, so you don't need to re-verify these:

- URL routing (`POST /<project>/<view>`).
- Two-candidate template resolution
  (`<view>.hbs` → `hbs/<view>.hbs`).
- Content-negotiation signals (`type: json` header,
  `Accept: application/json`, opportunistic JSON MIME upgrade,
  `text/html` fallback).
- Every shipped sample DSL renders the same output — the compat
  corpus at `compat/js-DSL/` is executed against the Rust engine
  on every CI run.
- `/healthz` payload shape (`{"service":"DataMapper","ok":true,"ts":…}`).
- Empty request body treated as `{}`.

---

## §7 If something breaks

- Check the boot log — most compat issues emit an actionable
  `WARN` or `INFO` line pointing here.
- Cross-check against
  [Failure modes](./failure-modes.md) — every HTTP status the
  Rust binary can emit is enumerated.
- File an issue at
  [github.com/turnerrainer/datamapper](https://github.com/turnerrainer/datamapper/issues)
  with the boot log + the request/response pair.
