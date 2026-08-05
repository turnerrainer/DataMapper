# DataMapper

Handlebars-templated payload shaping layer. Rust re-implementation
of [Buerostack/DataMapper](https://github.com/Buerostack/DataMapper).

**Version:** 0.1.0-alpha.2 · **License:** Apache-2.0
· **Docs:** [turnerrainer.github.io/datamapper](https://turnerrainer.github.io/datamapper/)
· **Images:** `docker.io/turnerrainer/datamapper:alpha`, `ghcr.io/turnerrainer/datamapper:alpha`

Drop `.hbs` files under `DSL/<project>/<view>.hbs` → they become
`POST /<project>/<view>` endpoints that shape a JSON request body
into a JSON (or HTML fallback) response. That's the whole product.

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

## Documentation

- **Book** — [turnerrainer.github.io/datamapper](https://turnerrainer.github.io/datamapper/)
  (getting started, config, failure modes)
- **Design** — [`docs/DESIGN.md`](./docs/DESIGN.md) — what DataMapper does and why
- **Standards** — [`STANDARDS.md`](./STANDARDS.md) — every generic
  build/docs/test/publish rule the project meets
- **Changelog** — [`CHANGELOG.md`](./CHANGELOG.md)
- **Original Node.js DataMapper** — <https://github.com/Buerostack/DataMapper>
