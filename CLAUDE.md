# CLAUDE.md — orientation for LLM contributors

Read this file before touching anything. It exists because
DataMapper's public-facing docs describe what the product does, and
this file describes what a code-writing agent needs to know to not
break it.

**Current release**: `v0.1.3-alpha` — security-hardening pass
(h2ck.me v1 audit, 5/5 findings closed). See
[`CHANGELOG.md`](./CHANGELOG.md) for the full delta.

**Trunk**: `dev`. There is no `main` branch on origin; releases
tag off `dev` after review.

**Verification set — every command exits 0 before you commit:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast     # 66 passed / 0 failed on v0.1.3-alpha
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

**Refuses to start?** — no. Local dev loops legitimately want a
writable tree. The WARN is loud; treat it as a hard failure in
staging/prod checklists.

**Code** — `src/main.rs` `warn_on_writable_dsl_root` +
`is_writable_by_us`.

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
| Handoff runbook, publishing steps, h2ck.me pipeline | [`HANDOFF.md`](./HANDOFF.md) |
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
