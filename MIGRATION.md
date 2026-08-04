# MIGRATION — DataMapper JS → DataMapper-on-Rust

**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md §7.2, §9.3`.
**Applies to**: operators porting a `Buerostack/DataMapper` (JS,
`v1.0.0`) deployment to `Buerostack/DataMapper-on-Rust` (Rust,
`v0.1.0-alpha.1`+).

Cross-index: every section here links to the corresponding
`DIVERGENCES.md` entry (`D-nnn`). Read the relevant divergence entry
for the "why"; use this file for the "how."

---

## §Configuration mapping

Given a JS `application.yml`-equivalent (there wasn't one, config was
env-only), here is what to move where.

| JS knob | Where it lived | Rust equivalent | Migration action |
|---|---|---|---|
| `PORT` env var | `docker-compose.yml` / systemd env | `port:` key in `datamapper.yaml` (or keep `PORT` env var) | Keep-as-is: the Rust binary reads `PORT` as fallback (D-001). New deployments SHOULD move to `datamapper.yaml`. |
| Inbound body limit `"2mb"` | `express.json({limit:"2mb"})` in `server.js` | `limits.max_request_bytes: 2097152` | Keep-as-is. Rust default is 2 MiB binary; JS was 2 000 000 decimal (D-002). Delta is ~5%, no realistic client is threading that needle. |
| `views/` template root | `app.set("views", [...])` in `server.js` | `dsl_path:` key in `datamapper.yaml` | Move `views/<project>/<view>.hbs` files under `dsl_path/`. Boot log warns if `./views/` still exists (D-003). |
| `views/layouts/` | `layoutsDir:` option | — | Inline any layout content into the template. Layouts are not supported (D-004). |

## §DSL template migration

Zero-touch for the shipped-JS sample corpus except:

### `.length` accessor — auto-rewritten, warned

JS Handlebars:

```handlebars
{ "total": {{#if products}}{{products.length}}{{else}}0{{/if}} }
```

Rust handles this transparently: on load, the template is scanned for
`.length` accessors; `{{arr.length}}` is rewritten to `{{len arr}}`,
and `{{#if arr.length}}` is rewritten to `{{#if arr}}`. A `warn!` log
line names the template so you can update the file at your leisure.

Recommended migration (silences the warn):

```handlebars
{ "total": {{#if products}}{{len products}}{{else}}0{{/if}} }
```

See `DIVERGENCES.md#D-010` for the rewriter's exact behaviour.

### `layout: false` sibling not present in `this`

If a template does `{{{json this}}}`, the JS output is
`{"layout":false, ...requestBody}`; the Rust output is
`requestBody` verbatim. Templates that referenced `{{layout}}` in
their body (none in the shipped corpus) must remove that reference.
`DIVERGENCES.md#D-015`.

### Handlebars helpers

| Helper | JS | Rust | Migration |
|---|---|---|---|
| `{{now}}` | `new Date().toISOString()` | `chrono::Utc::now().to_rfc3339()` | Consumers parsing the timestamp must accept `+00:00` offset in addition to `Z` suffix. `DIVERGENCES.md#D-008`. |
| `{{{json obj}}}` | `JSON.stringify(obj)` | `serde_json::to_string(obj)` | Missing keys serialise as `null` in Rust, empty string in JS. Guard optional fields with `{{#if}}`. `DIVERGENCES.md#D-009`. |
| `{{len obj}}` | (n/a — use `.length`) | Length of array/string/object; `0` on missing | New helper. See D-010 above. |

## §HTTP surface changes

### Routes

| Method + Path | JS | Rust | Notes |
|---|---|---|---|
| `POST /<project>/<view>` | Renders `DSL/<project>/<view>.hbs` or `DSL/<project>/hbs/<view>.hbs` | Same | No change. |
| `GET /healthz` | `{service, ok, ts}` | Same | No change. |
| `GET /health` | 404 | `{service, ok, ts}` | New alias (D-101). Old JS `/health` monitors that expected 404 now get 200. |
| `HEAD /healthz` `HEAD /health` | 200 (express default) | 200 | Parity. |
| `POST /healthz` | 405 `{"error":"MethodNotAllowed"}` | 405 `{"error":"MethodNotAllowed","message":"…"}` | Body adds `message`. `error` field unchanged. `DIVERGENCES.md#D-014`. |
| Non-POST on `/:project/*view` | express default 404 (HTML) | 405 with `allow: POST` | `DIVERGENCES.md#D-006`. |
| Path traversal `POST /samples/..%2fetc%2fpasswd` | Silently sanitised → normal 404 | 400 `{"error":"InvalidPath"}` | `DIVERGENCES.md#D-013`. |
| `POST` with `Content-Type: application/x-www-form-urlencoded` | Body parsed, template rendered from KV pairs | 415 `{"error":"UnsupportedContentType"}` | `DIVERGENCES.md#D-007`. Switch client to JSON body. |

### Error response bodies

Every error response gains a `"message"` field for human readers. The
machine-facing `"error"` field is unchanged. Grep patterns keyed on
`"error":"<code>"` still work. `DIVERGENCES.md#D-005`.

New error codes added (each has a JS predecessor that was HTML or unstructured):

- `InvalidJson` (400) — was express default HTML 400 (D-109)
- `InvalidPath` (400) — was silent-strip on JS side (D-013)
- `RequestTooLarge` (413) — was express default HTML 413 (D-110)
- `ResponseTooLarge` (500) — no JS equivalent (D-103)
- `Internal` (500) — no JS equivalent (D-111)
- `UnsupportedContentType` (415) — was silently accepted as form data on JS (D-007)

## §Container migration

### Image

The shipped Rust image is at:
- `docker.io/turnerrainer/datamapper:0.1.0-alpha.1`
- `ghcr.io/turnerrainer/datamapper:0.1.0-alpha.1`

Both are multi-arch (amd64 + arm64), Trivy-scanned, cosign-signed.

### Compose

```yaml
# Rust
services:
  datamapper:
    image: docker.io/turnerrainer/datamapper:0.1.0-alpha.1
    ports:
      - "3000:3000"
    volumes:
      - ./DSL:/app/DSL:ro          # NOTE mount path
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
- **Mount path** — `/workspace/app/DSL` → `/app/DSL` (`DIVERGENCES.md#D-018`).
- Hardening flags added (`DIVERGENCES.md#D-113`).
- `datamapper.yaml` is a new mount point for runtime config.

## §Log / monitor migration

### Boot log

Rust emits both the JS-compat single line AND structured tracing
output. Log-grep monitors keyed on:

```
DataMapper listening on :3000
```

continue to work. `DIVERGENCES.md#D-016`.

### Structured logs

New in Rust — human-readable `tracing` output. See `RUST_LOG` env var
in the config docs for filter syntax.

## §Rollback plan

If Rust behaviour is unacceptable in a specific slot, the JS image
still lives at `datamapper:latest` (or whatever tag the operator has
been using pre-migration). The mount paths, endpoints, and payload
shapes are byte-compatible except for the divergences catalogued in
`DIVERGENCES.md`. A rollback is: swap the image, adjust the mount
path back to `/workspace/app/DSL`, keep the DSL tree.

## §Operator porting checklist

- [ ] Set `port:` in `datamapper.yaml` (or keep `PORT` env var).
- [ ] Move `views/<project>/<view>.hbs` → `DSL/<project>/<view>.hbs`
      if any lived under `views/`.
- [ ] Update `docker-compose.yml` mount target from
      `/workspace/app/DSL` → `/app/DSL`.
- [ ] Optional: rewrite `{{arr.length}}` → `{{len arr}}` and
      `{{#if arr.length}}` → `{{#if arr}}` in `.hbs` files. Warns
      until fixed; renders correctly regardless.
- [ ] If any monitor asserts strict-equality on error JSON, adjust
      to allow the extra `"message"` field.
- [ ] If any client posts `application/x-www-form-urlencoded`,
      switch to `application/json`.
- [ ] If any consumer parses `{{now}}` output with a strict `Z`
      suffix regex, relax to `Z|(+\d{2}:\d{2})`.
