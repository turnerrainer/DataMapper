# Failure modes

Every failure DataMapper can surface, in one table. The `error`
field in the JSON body is the stable machine identifier;
`message` is a human string and may change between versions.

## Error codes

| HTTP status | `error` code | Cause | What to do |
|---|---|---|---|
| 400 | `InvalidJson` | Request body was not valid JSON. | Fix the client. Empty body is allowed and treated as `{}`. |
| 400 | `InvalidPath` | Route contained `..`, an absolute prefix, a null byte, or an empty segment. | Fix the client. Segments must be plain names. |
| 404 | `TemplateNotFound` | No file at either candidate path. Body includes `tried: [<paths>]` (each entry capped at 256 chars to defeat URL-length amplification). | Confirm the template exists at `DSL/<project>/<view>.hbs` or `DSL/<project>/hbs/<view>.hbs`. |
| 404 | `NotFound` | Global fallback — the URL didn't match any registered route (e.g., path-encoded traversal `%2F..%2F..%2Fetc%2Fpasswd` after Axum decoding). Same structured JSON shape as `TemplateNotFound`. | Fix the client URL. |
| 405 | `MethodNotAllowed` | POST on `/healthz` or `/health`, or non-POST on `/<project>/*`. | Use `GET`/`HEAD` for health, `POST` for template routes. |
| 408 | `RequestReadTimeout` | Client took longer than `limits.request_timeout_secs` to send the request body (slow-drip / slow-loris). Body includes `deadline_secs: <seconds>`. | Fix the client's send loop or raise the deadline. |
| 413 | `RequestTooLarge` | Body exceeded `limits.max_request_bytes`. Body includes `limit: <bytes>`. | Shrink the payload or raise the cap in `datamapper.yaml`. |
| 413 | `RequestArrayTooLarge` | Request-body JSON contains an array whose length exceeds `limits.max_body_array_length` (default 10 000). Body includes `length: <count>` and `limit: <cap>`. Bounds `{{#each}}` amplification before render. | Shrink the array, restructure the body, or raise the cap. Set to `0` to disable. |
| 415 | `UnsupportedContentType` | Request body was non-empty but Content-Type is not `application/json`, `text/json`, or `application/…+json`. Common trigger: `application/x-www-form-urlencoded`. The offending type is echoed in `message`. | Post JSON with `Content-Type: application/json`. See [Porting from JS DataMapper §3](./porting-from-js.md#request-content-type). |
| 431 | `HeaderValueTooLarge` | A single request-header value exceeded 8 KiB. Body includes `actual: <bytes>` and `limit: <bytes>`. Header names are NOT echoed (log-hygiene). | Shrink the header, or if legitimate raise the cap at the reverse proxy — 8 KiB matches nginx / Cloudflare defaults and covers legit JWTs, session cookies, and W3C traceparent. |
| 500 | `TemplateRenderError` | Handlebars failed to render. Body includes `view: <path>`. | Fix the template. Common causes: missing helper, typo in a `{{#if}}` block. |
| 500 | `ResponseTooLarge` | Rendered output exceeded `limits.max_response_bytes`. Body includes `limit: <bytes>`. | Fix the template (template amplification) or raise the cap. |
| 500 | `Internal` | Unexpected server-side error — I/O reading the template, config parse failure at startup, etc. | Check server logs (`RUST_LOG=debug` for detail). |
| 504 | *(no `error` code — empty body)* | Whole-request timeout via `TimeoutLayer` exceeded `limits.request_timeout_secs`. Fires when render + response-write together outrun the deadline. Slow-body scenarios surface as `408 RequestReadTimeout` instead (see above). | Fix the template (runaway recursion, extreme nesting) or raise the cap. |

## Response body shape

Every error response is JSON:

```json
{
  "error": "TemplateNotFound",
  "message": "template not found: tried [\"myproj/greet.hbs\", \"myproj/hbs/greet.hbs\"]",
  "tried": ["myproj/greet.hbs", "myproj/hbs/greet.hbs"]
}
```

Extra fields present depending on the variant:
- `tried: [<paths>]` — on `TemplateNotFound` (each entry capped at 256 chars).
- `limit: <bytes>` — on `RequestTooLarge` / `ResponseTooLarge` / `RequestArrayTooLarge` / `HeaderValueTooLarge`.
- `length: <count>` — on `RequestArrayTooLarge` (the offending array's element count).
- `actual: <bytes>` — on `HeaderValueTooLarge` (the offending header's byte length).
- `deadline_secs: <seconds>` — on `RequestReadTimeout`.
- `view: <path>` — on `TemplateRenderError`.

## Response `Content-Type` on non-error responses

Success (`200`) responses negotiate their `Content-Type` in this
order:

1. If the client signalled a JSON preference (`type: json` header,
   `Accept: application/json`, or `Accept: */*`) → try to parse the
   rendered output as JSON; on success serve as `application/json`,
   on failure serve raw with `application/json` still set.
2. Otherwise, if the rendered output parses as JSON →
   `application/json`.
3. Otherwise, if the client explicitly sent `Accept: text/html` →
   `text/html; charset=utf-8`.
4. Otherwise → `text/plain; charset=utf-8`.

Step 4 is the M2 defence: a mis-authored template whose output is
not valid JSON, served to a client that did not opt in to HTML,
lands as `text/plain` so a browser cannot execute any markup that
leaked into the response. Templates whose author intends HTML
should force the correct `Content-Type` at the reverse proxy, or
their callers should send `Accept: text/html` explicitly.

Two further tightenings apply on top of the base M2 rule:

- **RFC 7231 `;q=` quality values are respected** by
  `accepts_html` / `wants_json`. `Accept: text/html;q=0`
  (explicit exclusion) does NOT enable HTML fallback; `Accept:
  application/json;q=0, text/html` correctly selects HTML.
- **Templates using `{{{X}}}` (non-`json` triple-brace, un-escaped
  output) are forced to `text/plain`** on the fallback path
  regardless of `Accept: text/html`. The legit `{{{json obj}}}`
  helper output is still valid JSON and upgrades to
  `application/json` via step 2. This closes the composite XSS
  lane (attacker `Accept: text/html` + author's `{{{userInput}}}`
  template).

## Boot-time hard failures

Some misconfigurations refuse to start the server. Recognisable by
`Error: …` on stderr, no `listening on` line:

| Error prefix | Cause | Fix |
|---|---|---|
| `` config … line N: `cors_origin` is not implemented `` | Deprecated key in `datamapper.yaml`. | Remove the key and terminate CORS at the reverse proxy. |
| `parsing config …:` (with `[value elided to prevent config-content leak]` in the message) | `--config` pointed at a non-YAML file. The content is redacted from the log to protect against accidentally passing a secret file. | Point `--config` at the actual YAML. Location (line, column) is preserved in the diagnostic. |
| `REFUSING TO START in Production/Staging/Test: N unsafe posture item(s)` | Env-safety §11.2 tripped in a non-dev environment. Currently detects: DSL root writable. | Fix each listed item, OR set `APP_ENV=dev` for local runs. |
| `REFUSING TO LOAD dev-fixture DSL in non-dev environment` | Env-safety §11.3: a `.hbs` under `dsl_path` matches a dev/mock/test pattern (`dev-login`, `mock-`, `-mock`, `/test/`, `example-`, `-example`, `/dev/`, `/mocks/`) and the environment is non-dev. | Remove the file from the prod build, OR set `APP_ENV=dev`. |

Use `datamapper doctor` to catch these before boot without side
effects — see the [Configuration chapter §2.1](./configuration.md#21-cli).

## What DataMapper deliberately does NOT do on failure

- **Retry.** DataMapper is stateless and idempotent — retry policy
  is a client concern.
- **Fall back to a different template.** The two-candidate lookup
  (`<view>.hbs` → `hbs/<view>.hbs`) is the only fallback; no
  "closest match" or "template hierarchy" beyond that.
- **Cache errored templates.** A fix to the template file is
  picked up on the next request (per-request read).
- **Emit stack traces or internal file paths** in error responses.
  Log detail lives in the server log (`RUST_LOG`), not the wire
  response.
