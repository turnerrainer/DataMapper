# CLAUDE.md — orientation for LLM contributors

Read this file before touching anything. It exists because
DataMapper's public-facing docs describe what the product does, and
this file describes what a code-writing agent needs to know to not
break it.

**Current release** on `dev`: `v0.2.0-alpha` (release-prep in
progress; PR open against `dev`). Ships the h2ck.me v1
audit-plus-break-test findings surfaced after `v0.1.3-alpha`
(N1, N2, N3, N5, N6, N7, N8, F-DM-1, FN-LOG-1..4) and five
`FLEET-STRONGHOLDS.md` patterns (§1.6 W3C traceparent
propagation, §5.1 default security response headers, §8.2 doctor
CLI subcommand, §11.2 env-safety posture gate, §11.3 dev-fixture
DSL gate).

**Previous release**: `v0.1.3-alpha` (2026-09-06).

**Semver bump rationale**: the release contains multiple
client-observable wire-shape changes (new status codes 408 / 413
/ 431, unified 404 JSON shape, five new default response headers,
`traceparent` + `x-trace-id` on every response, CLI grew
subcommands). Under 0.x semver, minor is de-facto major, so the
0.1.3 → 0.2.0 bump signals "review your caller before upgrade."
See "Upgrading from v0.1.3-alpha to v0.2.0-alpha" below for
step-by-step client-side and operator-side migration.

**Trunk**: `dev`. There is no `main` branch on origin; releases
tag off `dev` after review. **Never push `main` and never bump
`Cargo.toml`/`VERSION`/`CHANGELOG.md` release headers without
explicit maintainer approval** — release cadence is a
human-authorised operation.

**Verification set — every command exits 0 before you commit:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast     # 163 passed / 0 failed on `dev`
                              # (was 155 at v0.2.0-alpha,
                              #  66 at v0.1.3-alpha)
cargo audit --deny warnings
```

`fmt --check` is a CI gate on its own — `clippy` + `test` clean is
not sufficient.

---

## What DataMapper is

A Handlebars template-folder → REST-endpoint proxy. Drop
`DSL/<project>/<view>.hbs` on disk → `POST /<project>/<view>`
renders it against the request body. HTTP + template are the whole
surface. No persistence, no admin API, no CORS termination, no
auth.

Domain deep-dive: [`docs/DESIGN.md`](./docs/DESIGN.md). Operator
docs: [`book/src/`](./book/src/) (published to GitHub Pages).
Cross-project ruleset (authoritative): `../DEV-REQUIREMENTS.md`
(local-only, not in this repo).

---

## Wire-visible behaviours a change must not silently break

Every item here has a regression test — if you break it, `cargo
test` will fire. Listed so you don't propose a "cleanup" that
walks into one of these on purpose.

| Behaviour | Location | Why it matters |
|---|---|---|
| `POST /:project/*view` route shape | `src/router.rs` | JS-DataMapper wire compat. |
| `type: json` header + `Accept:` negotiation | `src/router.rs` `wants_json`/`accepts_html` | Client-facing contract. |
| Two-candidate template lookup (`<view>.hbs`, `hbs/<view>.hbs`) | `src/renderer.rs` | JS compat. |
| Non-strict Handlebars (missing keys → empty string) | `src/renderer.rs` | JS compat. |
| `.length` accessor auto-rewrite | `src/renderer.rs` `contains_dot_length_accessor` | JS DSLs port zero-touch. |
| `GET /healthz` + `GET /health` alias | `src/router.rs` | Log-grep monitors. |
| `DataMapper listening on :<port>` stdout line | `src/main.rs` | JS log-grep monitors. |
| `PORT` env var honoured iff config omits `port:` | `src/config.rs` `apply_port_env` | JS docker-compose compat. |

Full compat matrix and JS→Rust deltas:
[`book/src/porting-from-js.md`](./book/src/porting-from-js.md).

---

## Breaking-behaviour deltas since v0.1.0-alpha.2

These are the changes an operator upgrading from alpha.2 to
alpha.3 (`v0.1.3-alpha`) will observe on the wire or at boot.
None of them are opt-in; the audit determined the prior behaviour
was unsafe or misleading.

### 1. `Accept:` fallback is now `text/plain` (was `text/html`)

**What changed** — when a template's output does not parse as JSON
and the client did **not** send `Accept: text/html`, the response
is now `Content-Type: text/plain; charset=utf-8`. Previously it
was `text/html; charset=utf-8` for all non-JSON-preferring
clients, including `Accept: */*` and no `Accept` header at all.

**Why** — reflected-content-type vector: an operator error page
containing attacker-controlled input would be rendered as HTML by
a browser that sent `Accept: */*`. Explicit opt-in closes it.

**How to detect a caller relying on the old behaviour** — the
caller sends `Accept: */*` (or omits `Accept`) and expects HTML.
Fix: send `Accept: text/html` explicitly.

**Regression tests** — `src/router.rs` `accepts_html_*` (unit),
`tests/it_regression_refacto.rs` (integration).

### 2. `cors_origin:` in `datamapper.yaml` refuses to boot

**What changed** — the deprecated `cors_origin:` key is now a
hard startup error naming the offending file:line. Previously it
was a silent no-op behind a boot WARN.

**Why** — silent drop misled operators. Browsers still blocked
requests, operators thought CORS was configured, common
workarounds (`Access-Control-Allow-Origin: *` by hand) were
worse than the correct fix.

**Correct fix** — remove the key from yaml, terminate CORS at a
reverse proxy (nginx `add_header`, Traefik middleware, etc.).

**Code** — `src/config.rs` `find_cors_origin_key`.

### 3. `max_response_bytes` enforced mid-render, not post-render

**What changed** — the response-size cap fires the instant the
byte budget is exceeded, not after the full render buffer is
allocated. Amplification templates that used to allocate GiB of
RAM before the check now short-circuit at cap size.

**Why** — DoS via template amplification. A 32-nested-each loop
against a 10 KiB cap now fails after ~10 KiB of allocation, not
after the full expansion.

**Impact on a legitimate template** — none unless the template
routinely renders bigger than the cap; if so, raise
`limits.max_response_bytes` in yaml.

**Code** — `src/renderer.rs` `CappedWriter` +
`render_template_to_write`.

### 4. Writable DSL root emits boot WARN

**What changed** — DataMapper probes `dsl_path` at boot; if
writable by the process, emits a WARN listing the offending
directories.

**Why** — a writable DSL mount lets a filesystem-writer swap
`.hbs` for a symlink to any file the process can read (TOCTOU /
symlink swap). Production compose files mount `DSL:/app/DSL:ro`
already; the WARN catches deployments that deviated.

**Refuses to start?** — WARN-only on `v0.1.3-alpha`. On the
rollup (see below) this UPGRADES to refuse-to-start when
`APP_ENV` is non-dev.

**Code** — `src/main.rs` `is_dsl_root_writable` +
`is_writable_by_us`.

---

## v0.2.0-alpha deltas (shipped in the rollup merge)

The `feat/security-and-fleet-rollup-v1` merge into `dev` (merged
PR #18) added these wire-visible / operator-visible changes on
top of the v0.1.3-alpha behaviours above. Each has regression
tests; count went 66 → 155.

### R.1 — Access log middleware + plain-text log stream (FN-LOG-1/2/3)

Every request emits one INFO line:
`http_request_completed method=X route=Y status=Z duration_us=... trace_id=...`.
No headers, bodies, client IPs, or raw URIs are logged — matched
route pattern only. `tracing_subscriber` also emits plain-text
(no ANSI escapes) when stderr is not a TTY, so `docker logs` /
`journalctl` capture SIEM-clean bytes.

- **Detect**: `docker logs <container> | LC_ALL=C tr -cd $'\x1b' | wc -c` returns 0.
- **Code**: `src/access_log.rs` + `main.rs` tracing init.

### R.2 — Five default security response headers (FLEET §5.1)

Every response (200 / 404 / 413 / 500) now carries:
`Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`,
`Strict-Transport-Security: max-age=63072000; includeSubDomains; preload`,
`X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`,
`Referrer-Policy: no-referrer`. Handler-set values are preserved
(insert-if-absent semantics).

- **Code**: `src/security_headers.rs` — `HEADERS` const is the
  single source of truth; regression tests iterate over it.

### R.3 — W3C `traceparent` + `x-trace-id` response headers (FLEET §1.6)

Every response carries `traceparent: 00-<32-hex trace>-<16-hex span>-01`
and `x-trace-id: <trace>`. Inbound `traceparent` is inherited when
well-formed (version=00, 32-hex trace-id ≠ all-zeros); fresh
`uuid::v4` otherwise. Span id is ALWAYS regenerated — that's this
service's span, not the caller's.

- **New dep**: `uuid = "1"` with `default-features = false, features = ["v4"]`.
- **Code**: `src/traceparent.rs`.

### R.4 — Accept-header parsed per RFC 7231 (N2)

`accepts_html` / `wants_json` now honour `;q=` quality values.
`Accept: text/html;q=0` correctly excludes HTML; `Accept:
application/json;q=0, text/html` correctly selects HTML. M2's
"explicit opt-in only" rule holds — bare `*/*` still does NOT
enable the HTML fallback.

- **Code**: `src/router.rs::accept_qvalue` + `MatchSpecificity`.

### R.5 — Triple-brace templates forced to `text/plain` (N1)

Templates using `{{{X}}}` (non-`json` triple-brace) always serve
`Content-Type: text/plain` on the fallback path, regardless of
`Accept: text/html`. Closes the composite-XSS lane the audit's
M2 fix didn't reach. `{{{json obj}}}` (legit helper) still
serves as JSON.

- **Detect at boot**: WARN per template — see
  `warn_on_unsafe_raw_output_templates` in `main.rs`.
- **Code**: `src/renderer.rs::contains_unsafe_raw_output` +
  `RenderOutcome.has_unsafe_raw_output`; router forces text/plain.

### R.6 — Body-read deadline via `tokio::time::timeout` (N3)

`invoke()` now takes `Request` (not `Bytes`) and drives
`axum::body::to_bytes` under an explicit deadline
(`limits.request_timeout_secs`, was hard-coded to the layer's
30s). Slow-drip / slow-loris uploads produce a structured 408
`RequestReadTimeout`.

The `limits.request_timeout_secs` config field is now wired
end-to-end (was only used by the TimeoutLayer via the hard-code
30s constant). Operators tuning the field will see it take
effect on both the whole-request cap and the body-read cap.

- **Code**: `src/router.rs::invoke` +
  `AppState.request_timeout_secs`.

### R.7 — Request-body array-length cap (F-DM-1)

Before render, the router walks the parsed JSON depth-first and
refuses any array whose element count exceeds
`limits.max_body_array_length` (default 10 000). Response is
413 `RequestArrayTooLarge` with `{length, limit}`. Set the cap
to `0` to disable.

- **Code**: `src/router.rs::find_oversize_array`.

### R.8 — Per-header value size cap (N8)

Middleware `header_value_size_gate` refuses any request with a
single header value > 8 KiB. Response is 431 `HeaderValueTooLarge`
with `{actual, limit}`. Header names are NOT echoed in the
response body (log-hygiene).

- **Code**: `src/router.rs::header_value_size_gate` +
  `MAX_HEADER_VALUE_BYTES`.

### R.9 — 404 shape unified + tried-paths clipped (N5 + N6 + FN-LOG-4)

Two changes bundled:
- `TemplateNotFound.tried` entries clipped at 256 chars per entry
  (was unbounded → 4× URL-length amplification).
- Global fallback handler emits the same
  `{"error":"NotFound","message":"..."}` shape for
  path-encoded-traversal requests that used to hit Axum's
  empty-body default 404.

- **Code**: `src/error.rs::clip_tried_paths` +
  `not_found_fallback`.

### R.10 — Config parse errors redacted (N7)

`config::redact_serde_error` strips the file's content from
serde's `invalid type: string "..."` diagnostic before it lands
in the boot log. An operator who misconfigured `--config` at a
mounted secret file no longer leaks the secret to log
aggregators. Location (line, column) preserved; the actual value
is elided.

- **Code**: `src/config.rs::redact_serde_error`.

### R.11 — Env-safety gates (FLEET §11.2 + §11.3)

New `src/env_safety.rs` module. `Environment::from_env()` reads
`APP_ENV` / `ENVIRONMENT` / `DEPLOY_ENV`; unknown values
fail-safe to `Production`.

- **§11.2** — writable DSL root (was WARN-only) now REFUSES to
  boot in non-dev.
- **§11.3** — DSL loader refuses to load any `.hbs` whose path
  matches `dev-login`, `mock-`, `-mock`, `/test/`, `example-`,
  `-example`, `/dev/`, `/mocks/` when non-dev. Shipped
  `DSL/samples/` tree does NOT trip any pattern.

- **Boot error shape** in non-dev:
  ```
  REFUSING TO START in Production: 1 unsafe posture item(s):
    - dsl.root_writable: DSL root is writable ...
        fix: mount the DSL tree read-only ... OR set APP_ENV=dev
  ```

### R.12 — `datamapper doctor` subcommand (FLEET §8.2)

Pre-boot validation without side effects. Prints one line per
check with `[ok]/[warn]/[error]` prefix (grep-friendly). Exit
`0` when nothing above `Ok` fired, `1` on any error (or on any
warn with `--strict`).

```bash
$ datamapper doctor            # validate; exit 0 unless error
$ datamapper doctor --strict   # exit 1 on any WARN too
$ datamapper --config x.yaml doctor
$ datamapper serve             # explicit run (default)
$ datamapper                   # implicit serve
```

- **New dep**: `clap = "4"` with `derive` feature.
- **Code**: `src/doctor.rs` + `main.rs` (clap wiring).

---

## Upgrading from v0.1.3-alpha to v0.2.0-alpha

This section is written for an LLM (or code-writing agent) that
has been asked to migrate a caller / operator config / adjacent
service across the version boundary. Every subsection is
prescriptive: what to look for, what to change, and how to
verify the change stuck. Human summary lives in
[`CHANGELOG.md`](./CHANGELOG.md) `## [0.2.0-alpha]`.

### Client-side (HTTP callers of DataMapper)

Grep the caller for these patterns, apply the fix, add a test.

#### C1. Callers that parse response headers

**Look for**: any code that iterates response headers, hashes
them for a signature, snapshots them into a test fixture, or
asserts on the exact header count.

**Change**: response now carries **7 extra headers** on every
response (5 security + `traceparent` + `x-trace-id`).
Insert-if-absent semantics mean any handler-set value survives.

**How to apply**: if the caller has a `snapshot(response)` or
`assert response.headers.keys() == {…}`-style test, extend the
expected set with:

- `content-security-policy`
- `strict-transport-security`
- `x-frame-options`
- `x-content-type-options`
- `referrer-policy`
- `traceparent`
- `x-trace-id`

Prefer `header in response.headers` (subset assertion) over
`response.headers == {…}` (equality) so future header additions
don't cascade.

**Verify**: caller test suite green against a live
`v0.2.0-alpha` instance.

#### C2. Callers that parse 404 response bodies

**Look for**: `if response.status == 404: return response.text
== ""`-style checks, or hard-coded empty-body expectations for
"route did not match" 404s.

**Change**: DataMapper's global fallback (`not_found_fallback`)
now returns the same structured JSON as `TemplateNotFound`:

```json
{"error": "NotFound", "message": "not found: ...", "tried": []}
```

**How to apply**:

```diff
- if response.status_code == 404 and not response.text:
+ if response.status_code == 404:
+     body = response.json()
+     assert body.get("error") in ("NotFound", "TemplateNotFound")
```

Every DataMapper 404 is now JSON with an `error` field. Branch on
the `error` value if the caller needs to distinguish
route-not-registered (`NotFound`) from template-not-on-disk
(`TemplateNotFound`).

**Verify**: caller test that hits a non-registered path (e.g.
`POST /notaproject/notaview` where `notaproject/` doesn't exist)
asserts the JSON shape.

#### C3. Callers that expect `Accept: text/html` fallback with `Accept: */*`

**Already handled in v0.1.3-alpha (M2)**. Nothing to do here in
v0.2.0. If the caller sends `Accept: */*` and expects HTML, it
already broke in v0.1.3.

#### C4. Callers that use `Accept: text/html;q=0` intending to exclude HTML

**Look for**: `Accept: text/html;q=0, application/json` or
similar q=0 constructs.

**Change**: v0.1.3 ignored `;q=` values, so `text/html;q=0`
falsely enabled HTML fallback. v0.2.0 respects q=0 → correctly
excludes HTML.

**How to apply**: no client change needed — the caller now works
as originally intended. If a caller was relying on the bug (using
`text/html;q=0` to get HTML), fix the header to `Accept:
text/html` (no q-value).

#### C5. Callers of templates that use `{{{userInput}}}` (un-escaped)

**Look for**: template files under `DSL/**/*.hbs` containing
`{{{X}}}` where `X` is anything OTHER than `json`, `json_pretty`,
or a helper the operator has proven is HTML-safe.

**Change**: v0.2.0 forces `Content-Type: text/plain` for any
response rendered from such a template, even if the caller sends
`Accept: text/html`. Callers rendering these templates into a
browser context no longer trigger HTML parsing.

**How to apply**:

1. Grep DSL for the pattern: `grep -rn '{{{[^j]' DSL/`
   (rough filter — `{{{j` catches `{{{json …}}}`).
2. For each hit, decide: escape to `{{X}}` (double-brace,
   HTML-safe) or explicitly serve as HTML via reverse proxy
   `Content-Type` override.
3. If the template genuinely emits HTML, force the content type
   at the reverse proxy — the DataMapper safety fallback stays
   `text/plain`.

**Verify**: boot log emits one WARN per offending template.

#### C6. Callers that stream / slow-drip request bodies

**Look for**: callers that upload > `limits.request_timeout_secs`
worth of body over a slow connection.

**Change**: slow uploads now return **408 `RequestReadTimeout`**
(was: eventually 504). Body: `{"error": "RequestReadTimeout",
"deadline_secs": <n>}`.

**How to apply**: if the caller retries on 5xx, extend the retry
policy to include 408 (or fix the upload cadence).

#### C7. Callers that send oversize headers

**Look for**: any single request header > 8 KiB (JWTs, session
cookies, custom headers).

**Change**: 8 KiB cap; over-cap returns **431
`HeaderValueTooLarge`** with `{actual, limit}`. Header name is
NOT echoed.

**How to apply**: shrink the header, or if legitimate, raise the
cap at the reverse proxy AND update the DataMapper source
(`MAX_HEADER_VALUE_BYTES` in `src/router.rs`) if the operator
runs a custom build. There is no config knob for this in
v0.2.0 (defence is deliberately hard-coded).

#### C8. Callers that post giant JSON arrays

**Look for**: request bodies containing arrays with more than
10 000 elements at any nesting depth.

**Change**: over-cap returns **413 `RequestArrayTooLarge`** with
`{length, limit}`.

**How to apply**: chunk the request, or raise
`limits.max_body_array_length` in `datamapper.yaml`. Set to `0`
to disable entirely.

### Operator-side (deployment / infra changes)

#### O1. Env-safety refusal to boot

**Look for**: a production / staging / test deployment where
`APP_ENV` (or `ENVIRONMENT` / `DEPLOY_ENV`) is anything other
than `dev`.

**Change**: v0.2.0 REFUSES to boot in non-dev when the DSL root
is writable OR any `.hbs` under `dsl_path` matches a
dev-fixture pattern.

**How to apply**:

1. Verify DSL mount is `:ro` in `docker-compose.yml`:
   ```yaml
   - ./DSL:/app/DSL:ro
   ```
2. Grep the DSL tree for dev-fixture patterns:
   ```bash
   find DSL -name '*.hbs' | grep -E \
     'dev-login|mock-|-mock|/test/|example-|-example|/dev/|/mocks/'
   ```
3. Either remove the matching files from the production build OR
   set `APP_ENV=dev` (only appropriate for local dev).
4. Run `datamapper doctor --strict` to verify the boot posture
   before rolling out.

**Boot error text** (grep-target for log aggregators):

```
REFUSING TO START in Production: N unsafe posture item(s):
  - dsl.root_writable: DSL root is writable ...
```

#### O2. CLI positional arguments

**Look for**: systemd unit files, docker CMD, kubernetes
`command:` / `args:` that pass positional arguments to
`datamapper`.

**Change**: v0.2.0 uses clap subcommands. Bare `datamapper`
still works (defaults to `serve`), but positional args other
than `serve` / `doctor` break.

**How to apply**:

```diff
- ExecStart=/usr/local/bin/datamapper /etc/datamapper.yaml
+ ExecStart=/usr/local/bin/datamapper --config /etc/datamapper.yaml serve
```

Or the equivalent CMD in Dockerfile:

```dockerfile
- CMD ["datamapper"]
+ CMD ["datamapper", "serve"]
```

The `--config <path>` flag is a global argument, applied before
the subcommand.

#### O3. Pre-boot validation via `datamapper doctor`

**New capability**: before rolling out a config change to
production, run `datamapper doctor --strict` in the target
environment (same env vars, same yaml, same DSL tree). Exit 0
means clean; exit 1 means fix something before start.

**How to apply**: add a `preStart` / `initContainer` /
`OnFailure` hook that runs `datamapper doctor --strict` and
gates rollout on exit 0.

Example (kubernetes initContainer):

```yaml
initContainers:
  - name: datamapper-doctor
    image: turnerrainer/datamapper:v0.2.0-alpha
    command: ["datamapper", "--config", "/etc/datamapper.yaml", "doctor", "--strict"]
    volumeMounts: [...]
```

#### O4. `limits.request_timeout_secs` now bounds two things

**Look for**: `datamapper.yaml` with `limits.request_timeout_secs`
set to a custom value.

**Change**: the value is now applied to BOTH the whole-request
timeout (via `TimeoutLayer`) AND the body-read deadline (new in
v0.2.0). Previously the body read was implicitly bounded by the
30 s hard-code inside `TimeoutLayer` only.

**How to apply**: if operator set `request_timeout_secs` low to
speed up slow-body rejection, verify the setting is compatible
with legitimate slow-but-large-body clients. Slow-body now
gets its own dedicated 408 error code — the timeout is more
observable, not looser.

#### O5. Log format changes (SIEM parsers)

**Look for**: SIEM / log-aggregation pipelines that regex the
DataMapper stderr for tracing output.

**Change**: `tracing_subscriber` now emits **plain text (no ANSI
escapes)** when stderr is not a TTY. Regexes that stripped ANSI
in preprocessing are now no-ops (safe) — regexes that DEPENDED
on the ANSI codes as delimiters (unusual) break.

**Detect**: `docker logs <container> | LC_ALL=C tr -cd $'\x1b'
| wc -c` returns 0 in v0.2.0.

Also new: a per-request `http_request_completed` INFO line with
method / route (matched pattern only, NOT raw URI) / status /
duration_us / trace_id. Add a SIEM rule if the operator wants
per-request telemetry into their aggregation pipeline.

### LLM-side (when editing this codebase)

#### L1. Where the R-series code lives

| R# | Module | Key symbols |
|---|---|---|
| R.1 | `src/access_log.rs` | `access_log_middleware` |
| R.2 | `src/security_headers.rs` | `security_headers`, `HEADERS` const |
| R.3 | `src/traceparent.rs` | `traceparent`, `parse_inbound_traceparent` |
| R.4 | `src/router.rs` | `accept_qvalue`, `MatchSpecificity` |
| R.5 | `src/renderer.rs` | `contains_unsafe_raw_output`, `RenderOutcome.has_unsafe_raw_output` |
| R.6 | `src/router.rs` | `invoke` (Request extractor), `AppState.request_timeout_secs` |
| R.7 | `src/router.rs` | `find_oversize_array`, `AppState.max_body_array_length` |
| R.8 | `src/router.rs` | `header_value_size_gate`, `MAX_HEADER_VALUE_BYTES` |
| R.9 | `src/error.rs` | `clip_tried_paths`, `not_found_fallback`, `MAX_TRIED_PATH_CHARS` |
| R.10 | `src/config.rs` | `redact_serde_error` |
| R.11 | `src/env_safety.rs` | `Environment`, `PostureCheck`, `DevFixtureAction` |
| R.12 | `src/doctor.rs` + `src/main.rs` | `doctor::run`, clap `Cli` / `Command` |
| R.13 (Unreleased) | `src/router.rs` | `method_not_allowed_on_render_route` — structured 405 + `Allow: POST` on the render route, `Allow: GET, HEAD` on healthz (T-13) |
| R.14 (Unreleased) | `src/shutdown.rs` + `src/main.rs` | `shutdown::shutdown_signal` — SIGTERM / SIGINT / SIGHUP graceful drain via `axum::serve(...).with_graceful_shutdown(...)` (T-18) |

Middleware order in `router.rs::build()` — OUTER-first is
LAST-in-chain: `header_value_size_gate` → `access_log_middleware`
→ `security_headers` → `traceparent`. Reordering breaks the
header-cap-runs-before-anything-else guarantee (N8) and the
trace-id-in-access-log invariant (FN-LOG-4).

#### L2. Adding a new error variant

Follow the pattern for `HeaderValueTooLarge` / `RequestArrayTooLarge` /
`RequestReadTimeout` in `src/error.rs`:

1. Add variant to `DataMapperError` enum with `#[error]` message.
2. Add to `status()` returning the HTTP status code.
3. Add to `code()` returning the stable `error` string.
4. In `into_response()`, insert a `if let DataMapperError::X { .. } = &self` block that adds finding-specific fields to the JSON body (`limit`, `actual`, `length`, `deadline_secs`, …).
5. Add a row to `book/src/failure-modes.md` "Error codes" table.
6. Add "Extra fields present" entry under the response-body-shape §.
7. Write a regression test in `tests/it_end_to_end.rs` asserting status + error code + extra fields.

#### L3. Adding a new middleware

Wire it in `router.rs::build()` at the correct chain position
(see L1). If it sets response headers, use `insert_if_absent`
semantics (see `security_headers.rs`) so handler-set values
survive. If it must run BEFORE the handler can panic, wire
before `Router` — otherwise as a `.layer(from_fn(…))`.

#### L4. Bumping a limit / adding a config field

1. Add field to `Limits` struct in `src/config.rs` with a
   `serde(default = "…")` fallback.
2. Add corresponding field on `AppState` in `src/router.rs`.
3. Wire `AppState { …, new_field: cfg.limits.new_field }` in
   `src/main.rs::serve()`.
4. Add a row to `book/src/configuration.md` §3 field reference.
5. **Every test fixture that constructs `AppState` needs the
   new field** — grep `AppState {` in `tests/` and add the
   field to each site. Missing one = compile break, so this
   is caught early, but painful — do them all in one pass.

#### L5. When you break the wire shape (do it deliberately)

Any new client-observable change (new response header, new
status code, new error field, changed content-type semantics)
must:

- Add a `CHANGELOG.md` entry under `## [Unreleased]` in a
  `### Breaking` block.
- Add a regression test that would fail against the old shape.
- Update `book/src/failure-modes.md` if error-path.
- Update `book/src/configuration.md` if config-path.
- Update this CLAUDE.md "Upgrading from" section with detection +
  fix guidance.
- Bump the SemVer minor (0.x means minor = breaking).

---

## Rollup wire-shape summary (release note draft)

If you're helping the maintainer draft the release notes for
whatever the next tag will be, the operator-visible checklist:

- New response headers on EVERY response (5 security headers +
  `traceparent` + `x-trace-id`).
- New response status codes possible: `408 RequestReadTimeout`,
  `413 RequestArrayTooLarge`, `431 HeaderValueTooLarge` — all
  structured JSON with error/message/… fields.
- Boot may REFUSE to start on non-dev with weak posture (see R.11)
  — set `APP_ENV=dev` if the deployment intentionally deviates.
- `Accept: text/html` semantics tightened (see R.4) — RFC-compliant
  callers behave correctly; buggy `q=0` callers now go the right
  way.
- New config field `limits.max_body_array_length` (default 10 000);
  new subcommand `datamapper doctor`; new deps `uuid`, `clap` +
  transitives.

Every point above has at least one regression test; nothing in
the rollup landed without a pin.

---

## Best-practice config (production)

**Compose posture** — see [`docker-compose.yml`](./docker-compose.yml).
Non-negotiable flags: `read_only: true`, `cap_drop: [ALL]`,
`no-new-privileges:true`, DSL mount `:ro`. Change any of these and
one of the above audit findings re-opens.

**`datamapper.yaml`** — see the shipped
[`datamapper.yaml`](./datamapper.yaml). Defaults are conservative:
2 MiB inbound, 16 MiB rendered, 30 s per-render timeout, port 3000,
DSL root `./DSL`. Raise only when you have a template that
legitimately needs more; drop for low-memory containers.

**What to check on a new deployment**

1. `docker compose up -d && docker compose logs datamapper 2>&1 | grep -E 'WARN|ERROR'` — should print nothing.
2. `curl -fsS http://localhost:3000/healthz` — should return 200 JSON.
3. Boot log includes `DataMapper listening on :<port>` (JS-compat line).
4. `docker exec datamapper touch /app/DSL/foo` — should fail (`read-only file system`); if it succeeds, the `:ro` mount is missing.

## Problematic configs — how to spot them

| Symptom | Root cause | Fix |
|---|---|---|
| Startup error `` `cors_origin` is not implemented `` | Deprecated key in yaml | Remove key; terminate CORS at reverse proxy. |
| Boot WARN `DSL root is writable by the DataMapper process` | Missing `:ro` on DSL volume | Add `:ro` to compose mount. |
| 415 `UnsupportedContentType` on migrated clients | Client posts `application/x-www-form-urlencoded` | Switch to `application/json`. |
| 500 `ResponseTooLarge` on a template that used to work | Template renders past `max_response_bytes` | Fix template or raise cap in yaml (with a load-test justification). |
| 504 timeout on a template that used to work | Renders past `request_timeout_secs` | Same — fix template or raise cap. |
| `PORT` env ignored | Yaml sets `port:` explicitly | Remove `port:` from yaml (env wins only when yaml omits it) OR just set yaml. |
| Response returned as `text/plain` when caller expected HTML | Caller did not send `Accept: text/html` | Send `Accept: text/html` explicitly. |
| Boot error naming `.length` in a JS DSL | Auto-rewriter parsed but detected legacy syntax | No action — INFO/WARN only; the rewriter handles it. Hand-migrate to silence. |

---

## When you fix a bug

Follow the audit-cycle lessons in the user's global CLAUDE.md
(`~/.claude/CLAUDE.md`): trace the full call chain (not just the
function), grep for siblings before declaring "found N places",
write tests that try to BREAK the fix, and prefer end-to-end
tests at seams over unit tests that mirror the fix.

The h2ck.me v1 findings are all textbook examples of "locally
correct, seam-broken" bugs — the M2 fix landed only because the
audit re-traced the Accept-negotiation path end-to-end instead of
verifying `accepts_html()` in isolation. Do the same.

---

## Where to find more

| Topic | File |
|---|---|
| Product identity + build/publish rules | [`STANDARDS.md`](./STANDARDS.md) |
| Security posture + supply-chain guardrails | [`SECURITY.md`](./SECURITY.md) |
| Cross-project ruleset (authoritative) | `../DEV-REQUIREMENTS.md` (local-only) |
| Refacto ruleset | `../REFACTO-REQUIREMENTS.md` (local-only) |
| Domain design | [`docs/DESIGN.md`](./docs/DESIGN.md) |
| Full change history | [`CHANGELOG.md`](./CHANGELOG.md) |
| JS→Rust operator porting summary | [`book/src/porting-from-js.md`](./book/src/porting-from-js.md) |
| Operator failure-modes reference | [`book/src/failure-modes.md`](./book/src/failure-modes.md) |
| Configuration reference | [`book/src/configuration.md`](./book/src/configuration.md) |
| CI workflows | [`.github/workflows/`](./.github/workflows/) |
| Task tracking | [`tasks/`](./tasks/) |
