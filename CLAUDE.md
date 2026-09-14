# CLAUDE.md — orientation for LLM contributors

Read this file before touching anything. It exists because
DataMapper's public-facing docs describe what the product does, and
this file describes what a code-writing agent needs to know to not
break it.

**Current release** on `dev`: `v0.1.3-alpha` (last shipped).

**Pending on `dev`**: a large security + FLEET-STRONGHOLDS rollup
merged from `feat/security-and-fleet-rollup-v1` — closes every
h2ck.me v1 audit + break-test finding surfaced after `v0.1.3-alpha`
(N1, N2, N3, N5, N6, N7, N8, F-DM-1, FN-LOG-1..4) and adopts five
`FLEET-STRONGHOLDS.md` patterns (§1.6 W3C traceparent
propagation, §5.1 default security response headers, §8.2 doctor
CLI subcommand, §11.2 env-safety posture gate, §11.3 dev-fixture
DSL gate). **Version has NOT been bumped** — the maintainer owns
the release cut. See "Rollup deltas" below for the wire-visible
changes to review before the next release.

**Trunk**: `dev`. There is no `main` branch on origin; releases
tag off `dev` after review. **Never push `main` and never bump
`Cargo.toml`/`VERSION`/`CHANGELOG.md` release headers without
explicit maintainer approval** — release cadence is a
human-authorised operation.

**Verification set — every command exits 0 before you commit:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast     # 155 passed / 0 failed on the rollup
                              # (was 66 on v0.1.3-alpha)
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

## Rollup deltas (pending — not yet in a tagged release)

The `feat/security-and-fleet-rollup-v1` merge into `dev` adds
these wire-visible / operator-visible changes on top of the
v0.1.3-alpha behaviours above. Each has regression tests; count
went 66 → 155.

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
