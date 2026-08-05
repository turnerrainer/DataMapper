# Handlebars helpers

Every template can call these directly. See
[Sample DSLs](./samples.md) for end-to-end usage against the
shipped fixtures.

DataMapper runs `handlebars-rust` in **non-strict mode** — missing
keys render as empty strings so `{{#if}}` / `{{#unless}}` guards
work naturally.

---

## `{{now}}`

Emits the current UTC time in RFC 3339 (a.k.a. ISO 8601).

**Template:**

```handlebars
{ "ts": "{{now}}" }
```

**Sample invocation:**

```bash
curl -sS -X POST http://localhost:3000/samples/ping \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{}'
```

**Response:**

```json
{"service":"DataMapper","project":"samples","ok":true,"ts":"2026-08-05T12:00:00.123456789+00:00"}
```

The exact suffix (`+00:00` vs `Z`) and precision (millisecond vs
nanosecond) differ from the JS implementation — see
[Porting from JS DataMapper](./porting-from-js.md).

---

## `{{{json obj}}}`

Serialises the parameter as compact JSON. **Use the triple-brace
form** (`{{{ }}}`); the double-brace form HTML-escapes the quotes
and breaks downstream JSON parsers.

**Template:**

```handlebars
{{{json this}}}
```

`this` is the entire request body — one-line templates like
`echo.hbs` use it as a passthrough.

**Sample invocation:**

```bash
curl -sS -X POST http://localhost:3000/samples/echo \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"msg":"hello","n":42,"nested":{"a":1}}'
```

**Response:**

```json
{"msg":"hello","n":42,"nested":{"a":1}}
```

**Nested example** — emit an object under a wrapper key:

```handlebars
{ "input": {{{json this}}}, "at": "{{now}}" }
```

Missing / undefined values serialise as `null` (JS emits nothing).
Guard optional fields with `{{#if}}`.

---

## `{{len items}}`

Returns the length of an array, string, or object. Returns `0`
for `null` or missing values.

**Template:**

```handlebars
{ "total": {{len products}} }
```

**Sample invocation:**

```bash
curl -sS -X POST http://localhost:3000/samples/arrays/map_products \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"products":[{"sku":"A1","name":"Widget","price":19.9},{"sku":"B2","name":"Gadget","price":29.5}]}'
```

**Response (excerpt):**

```json
{"items":[...],"total":2}
```

**Applied to other value types:**

| Input | `{{len x}}` |
|---|---|
| `[]` | `0` |
| `[1,2,3]` | `3` |
| `""` | `0` |
| `"hello"` | `5` |
| `{}` | `0` |
| `{"a":1,"b":2}` | `2` |
| `null` or missing | `0` |

---

## `.length` accessor — auto-rewritten for compat

JS Handlebars resolves `{{arr.length}}` via the implicit JavaScript
`.length` property; `handlebars-rust` does not. DataMapper detects
`.length` accessors at template-load time and rewrites them
transparently:

| You write | DataMapper renders as if you wrote |
|---|---|
| `{{arr.length}}` | `{{len arr}}` |
| `{{#if arr.length}}...{{/if}}` | `{{#if arr}}...{{/if}}` |
| `{{#unless arr.length}}...{{/unless}}` | `{{#unless arr}}...{{/unless}}` |

A `warn!` log line fires per affected template so you know which
files still lean on the JS syntax. Rewriting them by hand silences
the warning; the semantics are identical either way.

For the full JS→Rust porting story, see
[Porting from JS DataMapper](./porting-from-js.md).

---

## Combining helpers

Helpers compose naturally with block helpers:

```handlebars
{
  "items": [
    {{#each products}}{{#if @index}},{{/if}}
      { "sku": "{{sku}}", "name": "{{name}}" }
    {{/each}}
  ],
  "total": {{len products}},
  "generated_at": "{{now}}"
}
```

`@index`, `@first`, `@last`, and `@key` are all provided by
handlebars-rust natively — no helper needed.
