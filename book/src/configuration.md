# Configuration

Three surfaces:

1. **`datamapper.yaml`** — the runtime config file (see §1).
2. **Command-line flags and environment variables** — deploy-time
   knobs (see §2).
3. **Boot-log diagnostics** — what DataMapper prints at startup and
   what each line means (see §3).

For the helpers a template can call, see the
[Handlebars helpers](./handlebars-helpers.md) chapter.

---

## 1. `datamapper.yaml`

Search order:

1. `--config <path>` CLI flag.
2. `DATAMAPPER_CONFIG` environment variable.
3. `./datamapper.yaml` in the working directory.
4. `./datamapper.yml` in the working directory.
5. Built-in defaults if nothing matches.

The image ships `datamapper.yaml` at `/app/datamapper.yaml`.
Override by bind-mounting your own
(`-v ./datamapper.yaml:/app/datamapper.yaml:ro`).

### 1.1 Full reference

```yaml
# TCP port the HTTP server binds on 0.0.0.0.
port: 3000

# Root directory scanned for Handlebars templates. Each file
# `<dsl_path>/<project>/<view>.hbs` becomes route
# `POST /<project>/<view>`.
dsl_path: ./DSL

# Resource ceilings. Overrun surfaces as a structured 413 (inbound
# body cap) or 500 (rendered-output cap) or 408 (slow-body body-read
# deadline) or 504 (whole-request timeout) or 413 with the specific
# `RequestArrayTooLarge` error code (body array cap).
limits:
  max_request_bytes: 2097152      # 2 MiB
  max_response_bytes: 16777216    # 16 MiB
  request_timeout_secs: 30        # whole-request + body-read deadline
  max_body_array_length: 10000    # 0 to disable
```

### 1.2 Field reference

| Field | Type | Default | Description |
|---|---|---|---|
| `port` | u16 | `3000` | TCP port the server binds on `0.0.0.0`. Can be overridden by the `PORT` env var if the config file omits this field. |
| `dsl_path` | path | `./DSL` | Root of the template folder tree. |
| `limits.max_request_bytes` | usize | `2 * 1024 * 1024` | Inbound body cap. Overflow → 413 `RequestTooLarge`. |
| `limits.max_response_bytes` | usize | `16 * 1024 * 1024` | Rendered output cap. Overflow → 500 `ResponseTooLarge`. |
| `limits.request_timeout_secs` | u64 | `30` | Wall-clock ceiling. Applied at **two** places: (1) whole-request timeout via `TimeoutLayer` (overrun → 504 Gateway Timeout); (2) body-read deadline around `to_bytes` (overrun → 408 `RequestReadTimeout`) to defeat slow-loris uploads. |
| `limits.max_body_array_length` | usize | `10000` | Max array element count anywhere in the request-body JSON, walked depth-first. Overflow → 413 `RequestArrayTooLarge` with `{length, limit}`. Set to `0` to disable. Bounds the `{{#each}}` amplification lane before render starts. |

Unknown top-level or nested field → hard parse error at boot. Typo
protection (`dsl-path:` for `dsl_path:` etc.) is enforced by
`#[serde(deny_unknown_fields)]`.

### 1.3 Runnable config samples

**Bind to a different port + custom DSL root:**

```yaml
port: 8080
dsl_path: /var/lib/datamapper/DSL
```

**Tight limits for a low-memory container:**

```yaml
port: 3000
dsl_path: ./DSL
limits:
  max_request_bytes: 65536      # 64 KiB inbound
  max_response_bytes: 524288    # 512 KiB rendered
  request_timeout_secs: 5
```

**Generous limits for large data-shaping workloads:**

```yaml
port: 3000
dsl_path: ./DSL
limits:
  max_request_bytes: 33554432    # 32 MiB inbound
  max_response_bytes: 268435456  # 256 MiB rendered
  request_timeout_secs: 120
```

---

## 2. CLI flags and environment variables

### 2.1 CLI

Two subcommands:

- `datamapper serve` (default when no subcommand given) — run the HTTP server.
- `datamapper doctor [--strict]` — validate config + DSL tree + environment
  without starting the server. Prints one line per check with a
  `[ok]/[warn]/[error]` prefix; exits `1` on any error (or on any
  warn when `--strict`). Useful in CI to catch bad configs before a
  deploy.

| Flag | Scope | Effect |
|---|---|---|
| `--config <path>` or `--config=<path>` | global | Absolute or relative path to a `datamapper.yaml`-shaped file. Wins over every other resolution step. |
| `--strict` | `doctor` only | Exit non-zero on any WARN, not just ERROR. |
| `-h` / `--help` | global | Print help. |
| `-V` / `--version` | global | Print version. |

**Example doctor output**:

```
[ok]    env: cwd = /app
[ok]    env: PORT env not set — using config / default
[ok]    env: DATAMAPPER_CONFIG env not set — using default search path
[ok]    config: loaded from ./datamapper.yaml
[ok]    config: port=3000 dsl_path=./DSL max_req=2097152 max_resp=16777216 req_to=30s
[ok]    dsl: 11 template(s) discovered under ./DSL
[warn]  dsl: dsl_path ./DSL is writable by this process — production
        deployments should mount read-only …

summary: 6 ok, 1 warn, 0 error
```

### 2.2 Environment variables

| Variable | Effect |
|---|---|
| `DATAMAPPER_CONFIG` | Absolute path to the config file. Wins over the default search order but yields to `--config`. |
| `PORT` | JS DataMapper compatibility. Applied iff the loaded config does not explicitly set `port:`. `PORT=8181 datamapper` binds on 8181. Unparseable values (e.g., `PORT=xyz`) fall back to the default and log a warning. |
| `RUST_LOG` | `tracing-subscriber` filter directive. `info` is the shipping default; `debug` adds per-request detail; `trace` includes handlebars-internal logging. |
| `APP_ENV` / `ENVIRONMENT` / `DEPLOY_ENV` | Env classification for the FLEET-STRONGHOLDS §11 safety gates (first non-empty wins). Values: `dev`/`development`/`local` (permits weak posture with WARN), `test`/`testing`/`ci`, `stage`/`staging`/`preprod`, `prod`/`production`/`live` (refuses to boot on unsafe posture — see §3.4). Missing or unknown values fail-safe to `Production`. |

**Example:**

```bash
# JS-style deployment (works unchanged):
PORT=8080 RUST_LOG=info datamapper

# Rust-native deployment:
datamapper --config /etc/datamapper/prod.yaml

# Debug-friendly local run:
RUST_LOG=debug datamapper --config ./datamapper.yaml
```

---

## 3. Boot-log diagnostics

DataMapper emits both **structured** tracing output and a
**JS-compatible** single-line boot notice on stdout, so operators
carrying log-grep monitors from JS DataMapper deployments don't need
to change anything.

### 3.1 Structured tracing (via `tracing-subscriber`)

```text
2026-08-05T12:00:00.000Z  INFO datamapper: datamapper v0.2.0-alpha starting
2026-08-05T12:00:00.001Z  INFO datamapper: loaded config from ./datamapper.yaml
2026-08-05T12:00:00.001Z  INFO datamapper: dsl_path=./DSL port=3000 max_request_bytes=2097152 max_response_bytes=16777216
2026-08-05T12:00:00.002Z  INFO datamapper: listening on 0.0.0.0:3000
```

When no config file is found:

```text
2026-08-05T12:00:00.001Z  INFO datamapper: using built-in defaults (no datamapper.yaml found)
```

### 3.2 JS-compatible line (on stdout)

```text
DataMapper listening on :3000
```

Preserved verbatim from the JS implementation so any monitoring
rule keyed on this string still fires.

### 3.3 Compatibility warnings you may see

These fire only when a specific migration hazard is detected. All
are safe to ignore during the initial port, but each points at a
concrete file / config the operator should update at their leisure.

**Legacy `views/` root:**

```text
WARN datamapper: found 2 .hbs file(s) under ./views/ — Rust DataMapper only serves templates from dsl_path (see book/src/porting-from-js.md)
```

**Ported JS DSLs using `.length`:**

```text
INFO datamapper: 3 template(s) under ./DSL use the JS `.length` accessor and are being auto-rewritten via the compat helper (see book/src/porting-from-js.md): samples/arrays/map_products.hbs, myproj/dashboard.hbs, ...
```

Whenever such a template is rendered, a per-template `warn!` also
fires the first time:

```text
WARN datamapper: template samples/arrays/map_products.hbs uses `.length` accessor; auto-rewriting to `(len …)` for JS DataMapper DSL compat — see book/src/porting-from-js.md
```

**Unparseable `PORT` env var:**

```text
WARN datamapper: PORT env var 'not-a-number' is not a valid u16 port number; ignoring (JS DataMapper compat)
```

The server continues to boot with the fallback port (either from
`datamapper.yaml` or the default 3000).

---

## 4. Request/response contract

- **Request body**: JSON. Empty body is accepted as `{}`. Invalid
  JSON → 400 `InvalidJson`.
- **Request Content-Type**: `application/json`, `text/json`, or any
  `application/…+json`. Anything else on a non-empty body → 415
  `UnsupportedContentType` naming the offending type. Missing
  header on a JSON body → accepted (parity with JS).
- **Response body**: whatever the template renders. See §5 for MIME
  negotiation.
- **Route shape**: `POST /:project/*view`. `<project>` and every
  segment of `<view>` are sanitised individually — traversal
  (`..`) and absolute paths are rejected at 400.

## 5. Output negotiation

| Request signal | Output MIME |
|---|---|
| `type: json` request header (any case) | `application/json` |
| `Accept:` contains `application/json` | `application/json` |
| `Accept:` contains `*/*` (no `text/html`) | `application/json` |
| No JSON preference, output parses as JSON | `application/json` (opportunistic upgrade) |
| No JSON preference, `Accept:` explicitly lists `text/html`, output does NOT parse as JSON | `text/html; charset=utf-8` |
| No JSON preference, no `Accept: text/html` opt-in, output does NOT parse as JSON | `text/plain; charset=utf-8` |

**The `text/html` fallback is explicit-opt-in as of `v0.1.3-alpha`**
(h2ck.me v1 M2). `Accept: */*` and missing `Accept:` no longer
count as an HTML opt-in — the fallback for those clients is
`text/plain`, so an operator error page containing attacker-influenced
input cannot be rendered as HTML by a wildcard-Accept client.

As of `v0.2.0-alpha`, two further tightenings apply on top of
the M2 explicit-opt-in rule:

- The `Accept:` parser now honours RFC 7231 `;q=<value>` quality
  weights. `Accept: text/html;q=0` (explicit exclusion) no longer
  turns the HTML fallback on; `Accept: application/json;q=0,
  text/html` correctly selects HTML. `Accept: */*` alone still
  does NOT enable the HTML fallback.
- Templates using `{{{X}}}` (non-`json` triple-brace, un-escaped
  output) always serve `Content-Type: text/plain` on the
  JSON-detection fallback path — REGARDLESS of `Accept:
  text/html`. The `{{{json obj}}}` helper output is valid JSON
  and still upgrades to `application/json`. This closes the
  composite-XSS lane (bad template + attacker `Accept` header)
  deterministically.

---

## 6. Response headers on every response

Every response (200, 404, 413, 500, 504, error paths) now carries
a fixed set of security + observability headers, added by
middleware. Handler-set values take precedence; middleware only
inserts if absent.

| Header | Value | Rationale |
|---|---|---|
| `Content-Security-Policy` | `default-src 'none'; frame-ancestors 'none'` | Deny subresource loads even if HTML markup ever slipped through the negotiation. |
| `Strict-Transport-Security` | `max-age=63072000; includeSubDomains; preload` | Two-year HSTS with preload; applied unconditionally so a downstream proxy sees a coherent posture. |
| `X-Frame-Options` | `DENY` | Redundant with the CSP `frame-ancestors` for modern browsers; still there for older ones. |
| `X-Content-Type-Options` | `nosniff` | Kills MIME-sniff fallback — DataMapper's `Content-Type` is authoritative. |
| `Referrer-Policy` | `no-referrer` | HTML templates never leak the referring URL. |
| `traceparent` | `00-<32 hex trace>-<16 hex span>-01` | W3C Trace Context. Inherited from an inbound `traceparent` when well-formed (version=00, 32-hex non-zero trace-id); freshly generated otherwise. This service's span-id is ALWAYS regenerated. |
| `x-trace-id` | `<32 hex trace>` | Duplicate of the traceparent trace-id, kept as a separate header because most operator log-grep patterns look for a bare hex id rather than parsing the traceparent shape. |

Access log — one line per completed request:

```
INFO http_request_completed method=POST route=/:project/*view status=200 duration_us=1234 trace_id=<hex>
```

Only the matched route pattern is logged (never the raw URI);
no headers, request/response bodies, or client IPs are logged.
`tracing-subscriber` emits plain-text (no ANSI escapes) when
stderr is not a TTY, so `docker logs` / `journalctl` stay
SIEM-clean.

---

## 7. Environment-aware safety gates

`APP_ENV` (or `ENVIRONMENT` / `DEPLOY_ENV`) is read at boot;
unknown values fail-safe to `Production`. In any environment
above `dev`, the following posture items refuse to boot instead of
warning:

- **DSL root writable** — the compose file mounts `DSL:/app/DSL:ro`
  by default; deviation aborts boot in non-dev.
- **Dev-fixture DSL files present** — any `.hbs` under `dsl_path`
  whose path matches `dev-login`, `mock-`, `-mock`, `/test/`,
  `example-`, `-example`, `/dev/`, `/mocks/`. Shipped
  `DSL/samples/` files do NOT trip any pattern. Set `APP_ENV=dev`
  to permit fixtures locally.

Refusal messages name each offending item + a concrete remediation:

```
Error: REFUSING TO START in Production: 1 unsafe posture item(s):
  - dsl.root_writable: DSL root is writable by the DataMapper
    process — a filesystem writer can swap a .hbs for a symlink
    to any process-readable file (TOCTOU).
      fix: mount the DSL tree read-only (compose:
           `DSL:/app/DSL:ro`) OR set APP_ENV=dev
```

Use `datamapper doctor` (see §2.1) to preview the checks without
starting the server.
