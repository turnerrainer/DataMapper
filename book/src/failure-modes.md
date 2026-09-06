# Failure modes

Every failure DataMapper can surface, in one table. The `error`
field in the JSON body is the stable machine identifier;
`message` is a human string and may change between versions.

## Error codes

| HTTP status | `error` code | Cause | What to do |
|---|---|---|---|
| 400 | `InvalidJson` | Request body was not valid JSON. | Fix the client. Empty body is allowed and treated as `{}`. |
| 400 | `InvalidPath` | Route contained `..`, an absolute prefix, a null byte, or an empty segment. | Fix the client. Segments must be plain names. |
| 404 | `TemplateNotFound` | No file at either candidate path. Body includes `tried: [<paths>]`. | Confirm the template exists at `DSL/<project>/<view>.hbs` or `DSL/<project>/hbs/<view>.hbs`. |
| 405 | `MethodNotAllowed` | POST on `/healthz` or `/health`, or non-POST on `/<project>/*`. | Use `GET`/`HEAD` for health, `POST` for template routes. |
| 413 | `RequestTooLarge` | Body exceeded `limits.max_request_bytes`. Body includes `limit: <bytes>`. | Shrink the payload or raise the cap in `datamapper.yaml`. |
| 415 | `UnsupportedContentType` | Request body was non-empty but Content-Type is not `application/json`, `text/json`, or `application/…+json`. Common trigger: `application/x-www-form-urlencoded`. The offending type is echoed in `message`. | Post JSON with `Content-Type: application/json`. See [Porting from JS DataMapper §3](./porting-from-js.md#request-content-type). |
| 500 | `TemplateRenderError` | Handlebars failed to render. Body includes `view: <path>`. | Fix the template. Common causes: missing helper, typo in a `{{#if}}` block. |
| 500 | `ResponseTooLarge` | Rendered output exceeded `limits.max_response_bytes`. Body includes `limit: <bytes>`. | Fix the template (template amplification) or raise the cap. |
| 500 | `Internal` | Unexpected server-side error — I/O reading the template, config parse failure at startup, etc. | Check server logs (`RUST_LOG=debug` for detail). |
| 504 | *(no `error` code — empty body)* | Render call exceeded `limits.request_timeout_secs`. | Fix the template (runaway recursion, extreme nesting) or raise the cap. |

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
- `tried: [<paths>]` — on `TemplateNotFound`.
- `limit: <bytes>` — on `RequestTooLarge` / `ResponseTooLarge`.
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
