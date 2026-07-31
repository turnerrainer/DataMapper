# Standards for DataMapper

This file names the DataMapper identity fields (§0) and
otherwise defers to the cross-project ruleset in
[`../DEV-REQUIREMENTS.md`](../DEV-REQUIREMENTS.md).

Any deviation from DEV-REQUIREMENTS lives here with a rationale;
today, there are none.

---

## 0. Product identity

| Variable | Value |
|---|---|
| Product name | `DataMapper` |
| Cargo crate name | `datamapper` |
| Binary name | `datamapper` |
| GitHub repo | `github.com/turnerrainer/datamapper` |
| Docker Hub image | `turnerrainer/datamapper` |
| GHCR image | `ghcr.io/turnerrainer/datamapper` |
| License | Apache-2.0 |
| Book title | `DataMapper` |
| First stable target | `v1.0.0` on `main` |
| Author | Rainer Türner |
| Upstream (Node.js origin) | `Buerostack/DataMapper` |

## 1. Deviations from DEV-REQUIREMENTS

**None.** DataMapper meets DEV-REQUIREMENTS §§ 1–15 as
written. If a subsequent decision diverges, that decision plus its
justification lands here (and in the commit message per
DEV-REQUIREMENTS front-matter rule).

## 2. Project-specific extras

Additions on top of DEV-REQUIREMENTS that other Buerostack Rust
projects don't necessarily need:

### 2.1 Wire-compat with the Node.js DataMapper

Preserved so existing DSL trees migrate with minimal churn:

- `POST /:project/*view` route shape.
- `type: json` custom header + `Accept:` negotiation.
- Two-candidate template lookup (`<view>.hbs`,
  `hbs/<view>.hbs`).
- `GET /healthz` (plus `GET /health` alias).
- Non-strict Handlebars mode — missing keys render as empty
  strings.

The only known compatibility fracture is
`{{foo.length}}` → `{{len foo}}`. JS Handlebars resolves
`.length` on JS arrays; `handlebars-rust` has no equivalent
implicit accessor, so DSLs migrate with a two-token
find-and-replace. See `docs/DESIGN.md` §6.6.

### 2.2 Read templates per request

The Node.js DataMapper re-read from disk on every request;
operators depend on the edit-refresh loop. Preserved. Any future
caching layer must be opt-in and OFF by default.
