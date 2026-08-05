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
# body cap) or 500 (rendered-output cap) or 504 (request timeout).
limits:
  max_request_bytes: 2097152      # 2 MiB
  max_response_bytes: 16777216    # 16 MiB
  request_timeout_secs: 30
```

### 1.2 Field reference

| Field | Type | Default | Description |
|---|---|---|---|
| `port` | u16 | `3000` | TCP port the server binds on `0.0.0.0`. Can be overridden by the `PORT` env var if the config file omits this field. |
| `dsl_path` | path | `./DSL` | Root of the template folder tree. |
| `limits.max_request_bytes` | usize | `2 * 1024 * 1024` | Inbound body cap. Overflow → 413. |
| `limits.max_response_bytes` | usize | `16 * 1024 * 1024` | Rendered output cap. Overflow → 500 `ResponseTooLarge`. |
| `limits.request_timeout_secs` | u64 | `30` | Wall-clock ceiling per render call. Overrun → 504 Gateway Timeout. |

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

| Flag | Effect |
|---|---|
| `--config <path>` or `--config=<path>` | Absolute or relative path to a `datamapper.yaml`-shaped file. Wins over every other resolution step. |

### 2.2 Environment variables

| Variable | Effect |
|---|---|
| `DATAMAPPER_CONFIG` | Absolute path to the config file. Wins over the default search order but yields to `--config`. |
| `PORT` | JS DataMapper compatibility. Applied iff the loaded config does not explicitly set `port:`. `PORT=8181 datamapper` binds on 8181. Unparseable values (e.g., `PORT=xyz`) fall back to the default and log a warning. |
| `RUST_LOG` | `tracing-subscriber` filter directive. `info` is the shipping default; `debug` adds per-request detail; `trace` includes handlebars-internal logging. |

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
2026-08-05T12:00:00.000Z  INFO datamapper: datamapper v0.1.0-alpha.2 starting
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
| No JSON preference, output does NOT parse as JSON | `text/html; charset=utf-8` |
