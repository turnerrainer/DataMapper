# DataMapper

Handlebars-templated payload shaping layer. Rust re-implementation
of [Buerostack/DataMapper](https://github.com/Buerostack/DataMapper).

**Version:** 0.1.3-alpha · **License:** Apache-2.0
· **Docs:** [turnerrainer.github.io/datamapper](https://turnerrainer.github.io/datamapper/)
· **Images:** `docker.io/turnerrainer/datamapper:alpha`, `ghcr.io/turnerrainer/datamapper:alpha`

Drop `.hbs` files under `DSL/<project>/<view>.hbs` → they become
`POST /<project>/<view>` endpoints that shape a JSON request body
into a JSON (or `text/plain` / `text/html`) response. That's the
whole product.

## One-command demo

```bash
docker run -d --name datamapper -p 3000:3000 turnerrainer/datamapper:alpha
curl -sS -X POST http://localhost:3000/samples/ping \
  -H 'content-type: application/json' \
  -H 'type: json' -d '{}'
```

Response: `{"service":"DataMapper","project":"samples","ok":true,"ts":"2026-..."}`

## Build from source

```bash
git clone -b dev https://github.com/turnerrainer/datamapper.git
cd datamapper
docker compose up -d --build
```

## Upgrading from 0.1.0-alpha.2 → 0.1.3-alpha

`v0.1.3-alpha` is a security-hardening pass (h2ck.me v1 audit,
5/5 findings closed) with **four wire-visible deltas**. Read
[`CLAUDE.md`](./CLAUDE.md#breaking-behaviour-deltas-since-v010-alpha2)
before deploying if you're upgrading.

Short version:

1. **Non-JSON fallback is `text/plain`** unless the client sent
   `Accept: text/html` explicitly. `*/*` no longer counts.
2. **`cors_origin:` in yaml refuses to boot** (was a silent
   no-op). Terminate CORS at your reverse proxy.
3. **Response cap fires mid-render.** Templates that used to
   render huge outputs before failing now short-circuit at cap
   size.
4. **Writable DSL root emits boot WARN.** Production compose
   files ship `DSL:/app/DSL:ro` already; the WARN catches
   deviations.

Full changelog: [`CHANGELOG.md`](./CHANGELOG.md).

## Documentation

- **Book** — [turnerrainer.github.io/datamapper](https://turnerrainer.github.io/datamapper/)
  (getting started, config, failure modes, JS→Rust porting)
- **Design** — [`docs/DESIGN.md`](./docs/DESIGN.md) — what DataMapper does and why
- **LLM contributor orientation** — [`CLAUDE.md`](./CLAUDE.md) — breaking-behaviour crib, best-practice configs, problematic-config triage
- **Security posture** — [`SECURITY.md`](./SECURITY.md) — vulnerability reporting, supply-chain guardrails
- **Standards** — [`STANDARDS.md`](./STANDARDS.md) — product identity + build/docs/test/publish rules
- **Changelog** — [`CHANGELOG.md`](./CHANGELOG.md)
- **Original Node.js DataMapper** — <https://github.com/Buerostack/DataMapper>
