# Configuration

Two surfaces:

1. **`datamapper.yaml`** — the runtime config file (see §1).
2. **Handlebars helpers** — what templates can call (see §2).

## 1. `datamapper.yaml`

Search order for the file:

1. `--config <path>` CLI flag.
2. `DATAMAPPER_CONFIG` environment variable.
3. `./datamapper.yaml` in the working directory.
4. `./datamapper.yml` in the working directory.
5. Built-in defaults if nothing matches.

The image ships `datamapper.yaml` at `/app/datamapper.yaml`. Override
by bind-mounting your own (`-v ./datamapper.yaml:/app/datamapper.yaml:ro`).

### 1.1 Full reference

```yaml
# TCP port the HTTP server binds on 0.0.0.0.
port: 3000

# Root directory scanned for Handlebars templates. Each file
# `<dsl_path>/<project>/<view>.hbs` becomes route
# `POST /<project>/<view>`.
dsl_path: ./DSL

# Resource ceilings. Overrun surfaces as a structured 413
# (inbound body cap) or 500 (rendered-output cap).
limits:
  max_request_bytes: 2097152      # 2 MiB
  max_response_bytes: 16777216    # 16 MiB
  request_timeout_secs: 30
```

### 1.2 Field reference

| Field | Type | Default | Description |
|---|---|---|---|
| `port` | u16 | `3000` | TCP port the server binds on `0.0.0.0`. |
| `dsl_path` | path | `./DSL` | Root of the template folder tree. |
| `limits.max_request_bytes` | usize | `2 * 1024 * 1024` | Inbound body cap. Overflow → 413. |
| `limits.max_response_bytes` | usize | `16 * 1024 * 1024` | Rendered output cap. Overflow → 500. |
| `limits.request_timeout_secs` | u64 | `30` | Wall-clock ceiling per render call. |

### 1.3 Environment variables

| Variable | Effect |
|---|---|
| `DATAMAPPER_CONFIG` | Absolute path to the config file. Wins over the default search order. |
| `RUST_LOG` | Tracing filter. `info` is the shipping default; `debug` adds per-request detail. |

## 2. Built-in Handlebars helpers

Every template can call these directly. See the individual
templates under `DSL/samples/` for end-to-end usage.

### 2.1 `{{now}}`

Emits the current UTC time in RFC 3339 (a.k.a. ISO 8601).

```handlebars
{ "ts": "{{now}}" }
```

Renders as `{ "ts": "2026-07-29T12:34:56.789Z" }`.

### 2.2 `{{{json obj}}}`

Serialises the parameter as compact JSON. Use the triple-brace
form (`{{{ }}}`) so the output is emitted raw — the double-brace
form would HTML-escape the quotes and break downstream JSON
parsers.

```handlebars
{{{json this}}}
```

`this` is the entire request body — one-line templates like
`echo.hbs` use it as a passthrough.

### 2.3 `{{len items}}`

Returns the length of an array, string, or object. Returns `0` for
`null` or missing values. JS Handlebars resolves `foo.length` on
arrays via JavaScript's implicit `.length` property;
`handlebars-rust` has no equivalent, so migrating a Node.js DSL
means rewriting `{{foo.length}}` → `{{len foo}}`.

```handlebars
{ "total": {{len products}} }
```

## 3. Sample DSL tree

The shipping image bakes eleven templates under `DSL/samples/`:

| Route | What it demonstrates |
|---|---|
| `POST /samples/ping` | `{{now}}` helper, JSON literal output |
| `POST /samples/echo` | `{{{json this}}}` passthrough |
| `POST /samples/objects/select_fields` | field-plucking with `{{#if}}` defaults |
| `POST /samples/arrays/map_products` | array iteration + `{{len}}` |
| `POST /samples/conditionals/include_optional` | `{{#if}}` / `{{#unless}}` composition |
| `POST /samples/config/from_kv_array` | index-based array access `foo.[0].value` |
| `POST /samples/users/create` | defaults via `{{#if x}}...{{else}}fallback{{/if}}` |
| `POST /samples/users/patch` | conditional field emission for partial updates |
| `POST /samples/strings/join_tags_csv` | `{{#each}}` with `@index` for join-style output |
| `POST /samples/transform/flatten_address` | `{{#with}}` to unnest a subtree |
| `POST /samples/advanced/nested_each_index` | 2D iteration with `@index` at both levels |

Every sample carries a Handlebars comment `{{!-- ... --}}` header
with the exact `curl` invocation to try it. Read the template file
itself for the pattern.

## 4. Request/response contract

- **Request body**: JSON. Empty body is accepted as `{}`. Invalid
  JSON → 400 `InvalidJson`.
- **Response body**: whatever the template renders. See
  §5 for MIME negotiation.
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
