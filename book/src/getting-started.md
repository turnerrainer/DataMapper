# Getting started

Install DataMapper, run the demo, add your first template, verify
end-to-end — one page, three commands each.

## 1. Install

Two paths. Pick one.

**A. Docker (fastest):**

```bash
docker run -d --name datamapper -p 3000:3000 \
  turnerrainer/datamapper:alpha
```

**B. Build from source:**

```bash
git clone -b dev https://github.com/turnerrainer/datamapper.git
cd datamapper
cargo build --release
./target/release/datamapper-on-rust
```

The server listens on `0.0.0.0:3000` by default (change with the
`port:` field in `datamapper.yaml`).

## 2. Verify it's alive

```bash
curl -fsS http://localhost:3000/healthz
```

Response:

```json
{"service":"DataMapper","ok":true,"ts":"2026-07-29T12:00:00Z"}
```

Any status other than `200` means the server didn't start —
inspect `docker logs datamapper` (or the direct process output for
source builds).

## 3. Call a shipped sample

The image bakes in eleven sample DSLs under `DSL/samples/`.

```bash
curl -sS -X POST http://localhost:3000/samples/echo \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"msg":"hello","n":42}'
```

Response:

```json
{"msg":"hello","n":42}
```

`echo.hbs` is one line: `{{{json this}}}` — the built-in `json`
helper serialises the current context (the request body) back out.

## 4. Add your first template

Templates live under `DSL/<project>/<view>.hbs` on disk. To edit
without rebuilding the image, bind-mount your own DSL tree over the
container's:

```bash
mkdir -p ./DSL/myproj
cat > ./DSL/myproj/greet.hbs <<'EOF'
{ "greeting": "Hello, {{name}}!", "at": "{{now}}" }
EOF

docker run -d --name datamapper -p 3000:3000 \
  -v "$PWD/DSL:/app/DSL:ro" \
  turnerrainer/datamapper:alpha
```

Now hit it:

```bash
curl -sS -X POST http://localhost:3000/myproj/greet \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"name":"Ava"}'
```

Response:

```json
{"greeting":"Hello, Ava!","at":"2026-07-29T12:34:56.789Z"}
```

## 5. What just happened

The URL `POST /myproj/greet` resolved to
`DSL/myproj/greet.hbs` on disk. The JSON body became the Handlebars
context. The rendered output parsed as JSON, so DataMapper returned
it with `Content-Type: application/json`. If it hadn't parsed,
DataMapper would have returned it as `text/html`.

Fallback lookup: if `<project>/<view>.hbs` isn't found, DataMapper
tries `<project>/hbs/<view>.hbs` next. Both are equally supported;
pick whichever fits your project layout.

## Next

- **[Configuration](./configuration.md)** — change the port, the
  DSL root, resource caps. Every helper the templates can call.
- **[Failure modes](./failure-modes.md)** — every status code
  DataMapper can return, in one table.
