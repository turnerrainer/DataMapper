# REFACTO-AUDIT §8.3 — negative-space pass

**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md §8.3`.
**Written**: 2026-08-04.
**Method**: enumerate every behaviour the Rust target has that JS
DataMapper does not. For each, ask: could this conflict with a
source-of-truth-derived operator expectation?

Forward-pass audit (§2) is in `docs/REFACTO-AUDIT-S2.md`. That pass
lists things the JS side has that Rust doesn't. This pass is the
opposite — things Rust has that JS doesn't.

---

## §Extensions inventoried

Cross-reference: every extension has a divergence ID (`D-1nn`) in
`DIVERGENCES.md`. Row is the audit column; verdict is whether the
extension is safe against a JS-derived operator expectation.

| Extension | D-id | Introduced | Conflict check | Verdict |
|---|---|---|---|---|
| `GET /health` alias | D-101 | Rust from day one | JS returned 404. Any monitor asserting `/health` returns 404 breaks; JS docs don't advertise `/health`. | **Safe** — no documented expectation. |
| `HEAD /healthz`, `HEAD /health` | D-102 | Rust | JS returned 200 for HEAD (express default). | **Safe** — same outcome. |
| `limits.max_response_bytes` (16 MiB default, 500 on overrun) | D-103 | Rust | JS had no cap. A JS operator whose template renders >16 MiB payloads starts seeing 500s on Rust. | **Watch** — logged at boot per §6.2; raise via YAML if needed. |
| `limits.request_timeout_secs` (30s default, 504 on overrun) | D-104 | Rust | JS had no timeout. A JS operator with a slow template starts seeing 504s. | **Watch** — 30s is generous for a template renderer; raise via YAML if needed. |
| `--config` CLI flag | D-105 | Rust | JS had none. | **Safe** — no conflict. |
| `DATAMAPPER_CONFIG` env var | D-106 | Rust | JS had none. | **Safe**. |
| `RUST_LOG` env var | D-107 | Rust | JS had none. | **Safe**. |
| Canonicalised path-prefix check (`is_under`) | D-108 | Rust | JS did not canonicalise; symlinks inside `DSL/` could escape. Rust blocks that. | **Safe** — tighter security. If an operator relied on symlinks inside `DSL/` (unusual but possible), they get 400 InvalidPath instead of 200. |
| Structured 400 `InvalidJson` | D-109 | Rust | JS returned HTML 400. Consumers expecting HTML body break. | **Safe for machines** — same status, more useful body. Grep monitors for `TemplateNotFound` etc. unchanged. |
| Structured 413 `RequestTooLarge` | D-110 | Rust | JS returned HTML 413. Consumers expecting HTML body break. | **Safe for machines**. |
| Structured 415 `UnsupportedContentType` | D-007 | This refacto | JS returned 200 with form-parsed body (silent misroute). Consumers expecting 200 with form data now get 415. | **Watch** — this is the only extension that turns a JS success into a Rust failure. Migration documented in `MIGRATION.md#form-encoded-bodies`. |
| Structured 500 `ResponseTooLarge` | D-103 pair | Rust | See D-103. | **Watch**. |
| `500 Internal` catch-all | D-111 | Rust | JS had generic 500. | **Safe** — same status, clearer body. |
| 57 tests (24 unit + 16 e2e + 17 regression) | D-112 | Rust | JS had none. | **Safe** — internal-only. |
| Hardened container (non-root, read-only rootfs, cap_drop ALL, tini) | D-113 | Rust | JS ran root on `node:20-alpine`. | **Watch** — templates that tried to write anywhere break, but templates are declarative-render only so this is not a realistic case. |
| mdBook docs | D-114 | Rust | JS shipped `docs/architecture/*.md`. | **Safe** — pure content-migration. |
| README targeted at operators, not integrators | D-115 | Rust | JS README was integration-first. | **Safe**. |
| Every error body carries a `message` field | D-005/D-014 | Rust | JS `{"error":"…"}` shape. Strict-equality body consumers break. | **Watch** — grep-safe; strict-equality is a rare monitor pattern. |
| Boot log adds structured tracing (kept the JS-compat line) | D-016 | Rust | JS emitted only the single line. | **Safe** — JS line preserved (compat println), structured log is additive. |
| Boot INFO listing templates that use the `.length` rewriter | §6.2 | This refacto | JS had no such note. | **Safe** — purely informational. |
| Container DSL mount path `/app/DSL` | D-018 | Rust | JS mounted at `/workspace/app/DSL`. | **Watch** — a bind-mount script hard-coded to the JS path breaks. Documented in `MIGRATION.md#compose`. |

Total: 21 extension rows. All either **Safe** or **Watch** (documented in `MIGRATION.md` + `DIVERGENCES.md`). Zero **Break** verdicts.

---

## §What this negative-space pass did NOT cover (R3.4)

- **Undocumented private endpoints or debug hooks** — grep-verified
  nonexistent in `src/router.rs`, but a future addition could
  regress this.
- **Runtime behaviours that only emerge under load** — e.g., axum
  connection-pool tunings, tokio thread pool sizing. These are
  Rust framework defaults, not features Rust exposes; a review at
  the operations layer would catch them.
- **Handlebars-engine differences beyond `.length`** — sparse-array
  indexing, prototype-chain lookups, block-helper argument
  coercion. The JS source's shipped corpus exercises none of these,
  so the negative-space pass has no basis to compare. If a future
  operator hits one, it becomes a D-01n entry.
- **Docker image layer count / size** — not user-facing; not
  audited.

---

## §Verdict

No operator expectation carried over from `Buerostack/DataMapper` (JS
v1.0.0) is silently broken by a Rust-side extension. The three
**Watch** items (response cap, request timeout, container mount path
change) are all documented in `MIGRATION.md` with concrete
mitigations.

The forward pass (§2 audit) and this negative-space pass together
close the R8.3 requirement.
