# Introduction

**DataMapper** turns a folder of Handlebars templates into a
REST API. Drop a file at `DSL/samples/ping.hbs` → it answers
`POST /samples/ping`. Send a JSON body → get the rendered output
back as JSON (or HTML, if that's what the template produces).

**Version:** 0.1.0-alpha.1 · **License:** Apache-2.0
· **Source:** [github.com/turnerrainer/datamapper](https://github.com/turnerrainer/datamapper)

## What DataMapper is for

DataMapper is the **payload-shaping layer** of the Buerostack stack.
It sits between backend service providers and payload consumers
(typically UIs and downstream services) and normalises heterogeneous
upstream responses into ready-to-use shapes. Consumers don't massage
JSON on the client — the shape they need is what they get on the
wire.

When an upstream changes its wire format, only the template
changes. No client rewrite, no schema migration, no coordinated
release.

## What DataMapper is NOT for

- **Fetching data from upstreams.** No outbound HTTP, no database
  connections. The request body IS the input. Aggregation belongs
  in Ruuter.
- **Business rules or auth.** Cross-cutting concerns live in
  Ruuter or TIM. DataMapper trusts its input.
- **Arbitrary code execution.** Handlebars is declarative on
  purpose.

See [`docs/DESIGN.md`](https://github.com/turnerrainer/datamapper/blob/dev/docs/DESIGN.md)
for the full non-goals list.

## One-command demo

```bash
docker run -d --name datamapper -p 3000:3000 \
  turnerrainer/datamapper:alpha

curl -sS -X POST http://localhost:3000/samples/ping \
  -H 'content-type: application/json' \
  -H 'type: json' -d '{}'
```

Response:

```json
{"service":"DataMapper","project":"samples","ok":true,"ts":"2026-07-29T12:34:56.789Z"}
```

Every helper (`{{now}}`, `{{{json obj}}}`, `{{len items}}`) is
usable in any template. Full reference: [Configuration](./configuration.md).

## Where to go next

- **[Getting started](./getting-started.md)** — install, first
  template, verify.
- **[Configuration](./configuration.md)** — every knob, every
  helper.
- **[Failure modes](./failure-modes.md)** — every HTTP status +
  error code + what to do about it.
