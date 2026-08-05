# Sample DSLs

Eleven templates ship under `DSL/samples/`. Each demonstrates one
common pattern. Every template file carries the exact `curl`
invocation to run it in its Handlebars comment header — this page
mirrors that content in one place so you can pick a pattern
without opening the file.

The image bakes all of these in. Point at your running container:

```bash
BASE=http://localhost:3000
```

---

## `POST /samples/ping` — liveness with helper timestamp

**File:** `DSL/samples/ping.hbs`

**Template:**

```handlebars
{ "service": "DataMapper", "project": "samples", "ok": true, "ts": "{{now}}" }
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/ping \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{}'
```

**Response:**

```json
{"service":"DataMapper","project":"samples","ok":true,"ts":"2026-08-05T12:00:00.123+00:00"}
```

---

## `POST /samples/echo` — passthrough

**File:** `DSL/samples/echo.hbs`

**Template:**

```handlebars
{{{json this}}}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/echo \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"msg":"hello","n":42,"nested":{"a":1}}'
```

**Response:**

```json
{"msg":"hello","n":42,"nested":{"a":1}}
```

---

## `POST /samples/objects/select_fields` — pluck fields with defaults

**File:** `DSL/samples/objects/select_fields.hbs`

**Template:**

```handlebars
{
  "id": {{#if user.id}}{{user.id}}{{else}}0{{/if}},
  "fullName": "{{#if user.first}}{{user.first}}{{else}}Unknown{{/if}} {{#if user.last}}{{user.last}}{{else}}User{{/if}}",
  "role": "{{#if role}}{{role}}{{else}}guest{{/if}}"
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/objects/select_fields \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"user":{"id":10,"first":"Ava","last":"Stone"},"role":"admin"}'
```

**Response:**

```json
{"id":10,"fullName":"Ava Stone","role":"admin"}
```

---

## `POST /samples/arrays/map_products` — array iteration + `{{len}}`

**File:** `DSL/samples/arrays/map_products.hbs`

**Template:**

```handlebars
{
  "items": [
    {{#each products}}{{#if @index}},{{/if}}{
      "sku": "{{this.sku}}",
      "title": "{{this.name}}",
      "price": {{this.price}}
    }{{/each}}
  ],
  "total": {{len products}}
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/arrays/map_products \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"products":[{"sku":"A1","name":"Widget","price":19.9},{"sku":"B2","name":"Gadget","price":29.5}]}'
```

**Response:**

```json
{"items":[{"sku":"A1","title":"Widget","price":19.9},{"sku":"B2","title":"Gadget","price":29.5}],"total":2}
```

---

## `POST /samples/conditionals/include_optional` — `{{#if}}` / `{{#unless}}`

**File:** `DSL/samples/conditionals/include_optional.hbs`

**Template:**

```handlebars
{
  {{#if phone}}"phone": "{{phone}}"{{#if email}},{{/if}}{{/if}}{{#if email}}
  "email": "{{email}}"{{/if}}{{#unless phone}}{{#unless email}} "note": "no contact details provided"{{/unless}}{{/unless}}
}
```

**Call — email only:**

```bash
curl -sS -X POST $BASE/samples/conditionals/include_optional \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"email":"a@x.io"}'
```

**Response:**

```json
{"email":"a@x.io"}
```

**Call — neither:**

```bash
curl -sS -X POST $BASE/samples/conditionals/include_optional \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{}'
```

**Response:**

```json
{"note":"no contact details provided"}
```

---

## `POST /samples/config/from_kv_array` — index-based array access

**File:** `DSL/samples/config/from_kv_array.hbs`

**Template:**

```handlebars
{
  "theme": "{{configuration.[0].value}}",
  "pageSize": "{{configuration.[1].value}}",
  "featureX": {{#if configuration.[2].value}}true{{else}}false{{/if}}
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/config/from_kv_array \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"configuration":[{"key":"theme","value":"dark"},{"key":"pageSize","value":"20"},{"key":"featureX","value":"true"}]}'
```

**Response:**

```json
{"theme":"dark","pageSize":"20","featureX":true}
```

---

## `POST /samples/users/create` — defaults via `{{else}}`

**File:** `DSL/samples/users/create.hbs`

**Template:**

```handlebars
{
  "username": "{{username}}",
  "email": "{{email}}",
  "active": {{#if active}}{{active}}{{else}}true{{/if}}
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/users/create \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"username":"neo","email":"neo@example.com"}'
```

**Response:**

```json
{"username":"neo","email":"neo@example.com","active":true}
```

---

## `POST /samples/users/patch` — conditional field emission

**File:** `DSL/samples/users/patch.hbs`

**Template (excerpt):**

```handlebars
{
  "id": {{id}}{{#if username}},
  "username": "{{username}}"{{/if}}{{#if email}}{{#if username}},{{/if}}
  "email": "{{email}}"{{/if}}...
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/users/patch \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"id":123,"username":"trinity"}'
```

**Response:**

```json
{"id":123,"username":"trinity"}
```

---

## `POST /samples/strings/join_tags_csv` — `{{#each}}` with `@index`

**File:** `DSL/samples/strings/join_tags_csv.hbs`

**Template:**

```handlebars
{
  "csv": "{{#each tags}}{{#if @index}},{{/if}}{{this}}{{/each}}"
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/strings/join_tags_csv \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"tags":["alpha","beta","gamma"]}'
```

**Response:**

```json
{"csv":"alpha,beta,gamma"}
```

---

## `POST /samples/transform/flatten_address` — `{{#with}}` scope

**File:** `DSL/samples/transform/flatten_address.hbs`

**Template:**

```handlebars
{
  "id": {{user.id}},
  "name": "{{user.name}}",
  {{#with user.address}}
  "street": "{{street}}",
  "city": "{{city}}",
  "zip": "{{postal}}",
  "country": "{{country}}"
  {{/with}}
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/transform/flatten_address \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"user":{"id":7,"name":"Ava","address":{"street":"Main 1","city":"Tallinn","postal":"10115","country":"EE"}}}'
```

**Response:**

```json
{"id":7,"name":"Ava","street":"Main 1","city":"Tallinn","zip":"10115","country":"EE"}
```

---

## `POST /samples/advanced/nested_each_index` — 2D iteration

**File:** `DSL/samples/advanced/nested_each_index.hbs`

**Template:**

```handlebars
{
  "rows": [
    {{#each matrix}}{{#if @index}},{{/if}}{
      "row": {{@index}},
      "cols": [{{#each this}}{{#if @index}},{{/if}}{{this}}{{/each}}]
    }{{/each}}
  ]
}
```

**Call:**

```bash
curl -sS -X POST $BASE/samples/advanced/nested_each_index \
  -H 'content-type: application/json' \
  -H 'type: json' \
  -d '{"matrix":[[1,2,3],[4,5,6],[7,8,9]]}'
```

**Response:**

```json
{"rows":[{"row":0,"cols":[1,2,3]},{"row":1,"cols":[4,5,6]},{"row":2,"cols":[7,8,9]}]}
```
