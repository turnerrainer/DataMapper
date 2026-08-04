# REFACTO-PORT-PLAN — Test-corpus port plan

**Written**: 2026-08-04.
**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md §1.3, §4.1, §4.2`.
**Scope**: every fixture / example / test that ships in the JS
source-of-truth `Buerostack/DataMapper/`.

The JS project has no formal test suite — it uses the shipped DSLs
themselves as smoke fixtures (each carries a curl invocation in a
Handlebars comment header). This plan therefore treats **every
sample DSL + its curl body as a fixture**, plus any runnable example.

Legend:
- **Port verbatim** — same fixture works identically in the target;
  target test asserts the same outcome.
- **Port with fixture edits** — target's contract intentionally
  differs from source; edits listed field-by-field.
- **Skip** — out of scope for target; rationale documented.

---

## §A DSL fixtures

Each row is one `.hbs` sample with its embedded curl body. The
"target test" column names the Rust integration test that
exercises the fixture end-to-end.

| Fixture | Body from curl comment | Plan | Target test | Notes |
|---|---|---|---|---|
| `DSL/samples/ping.hbs` | `{}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | Both impls emit JSON with `service`, `ok`, `ts`. |
| `DSL/samples/echo.hbs` | `{"msg":"hello","n":42,"nested":{"a":1}}` | Port with fixture edits | `it_end_to_end::renders_echo_and_returns_json`, `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | JS emits `{"layout":false,...body}` via `{{{json this}}}` (D-015). Target asserts body keys only, ignoring `layout`. |
| `DSL/samples/advanced/nested_each_index.hbs` | `{"matrix":[[1,2,3],[4,5,6],[7,8,9]]}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/arrays/map_products.hbs` | `{"products":[{"sku":"A1","name":"Widget","price":19.9},{"sku":"B2","name":"Gadget","price":29.5}]}` | Port with fixture edits | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | Rust body uses `{{len products}}` instead of `{{products.length}}` (D-010). |
| `DSL/samples/conditionals/include_optional.hbs` | `{"email":"a@x.io"}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/config/from_kv_array.hbs` | `{"configuration":[{"key":"theme","value":"dark"},{"key":"pageSize","value":"20"},{"key":"featureX","value":"true"}]}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/objects/select_fields.hbs` | `{"user":{"id":10,"first":"Ava","last":"Stone"},"role":"admin"}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/strings/join_tags_csv.hbs` | `{"tags":["alpha","beta","gamma"]}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/transform/flatten_address.hbs` | `{"user":{"id":7,"name":"Ava","address":{"street":"Main 1","city":"Tallinn","postal":"10115","country":"EE"}}}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/users/create.hbs` | `{"username":"neo","email":"neo@example.com"}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |
| `DSL/samples/users/patch.hbs` | `{"id":123,"username":"trinity"}` | Port verbatim | `it_end_to_end::all_shipped_samples_render_against_their_curl_bodies` | |

## §B Runnable examples

| Fixture | Plan | Target | Notes |
|---|---|---|---|
| `examples/basic-usage/README.md` + `package.json` | Port verbatim | `examples/basic-usage/` | Empty in Rust — see D-019, fixed in this refacto batch. |

## §C Integration flows not in JS but shipped in Rust

| Fixture | Plan | Target test | Notes |
|---|---|---|---|
| Empty request body → treated as `{}` | Port verbatim | `it_end_to_end::empty_body_becomes_empty_object` | JS: `express.json` rejects zero-length body silently, downstream template renders with `req.body === {}`. Rust: explicit branch (`router.rs:93`). Behaviour matches. |
| Invalid JSON body → structured 400 | Port with fixture edits | `it_end_to_end::invalid_json_body_returns_400` | JS returns unstructured 400 HTML; Rust returns `{"error":"InvalidJson"}`. D-109. |
| Body over limit → structured 413 | Port with fixture edits | `it_end_to_end::request_body_over_limit_returns_413` | JS returns unstructured 413 HTML; Rust returns `{"error":"RequestTooLarge","limit":<n>}`. D-110. |
| Rendered output over cap → 500 | Skip (no JS parity) | `it_end_to_end::response_body_over_limit_returns_500` | JS has no output cap. D-103. |
| `GET /health` alias | Skip (no JS parity) | `it_end_to_end::health_alias_works` | D-101. |
| Non-POST on `/:project/*` → 405 | Skip (no JS parity) | (n/a) | JS: express default 404. D-006. |
| Path traversal → 400 InvalidPath | Port with fixture edits | `it_end_to_end::path_traversal_is_blocked` | JS silently strips `..`; Rust rejects. D-013. |
| Content-negotiation via `type: json` custom header | Port verbatim | `it_end_to_end::renders_echo_and_returns_json` + unit tests in `router::tests` | |
| Content-negotiation via `Accept: application/json` | Port with fixture edits | `it_end_to_end::json_coercion_via_accept_header` + `router::tests::wants_json_via_accept` | Rust `Accept` parser differs from `req.accepts(...)`. D-011. |
| HTML fallback | Port with fixture edits | `it_end_to_end::html_fallback_when_output_not_json_and_no_json_preference` | Rust adds `charset=utf-8`. D-012. |
| Opportunistic JSON MIME upgrade | Port verbatim | `it_end_to_end::opportunistic_json_when_output_looks_like_json` | |
| `hbs/` subfolder fallback | Port verbatim | `it_end_to_end::fallback_hbs_subfolder_resolves` | |
| Template-not-found returns two `tried` paths | Port verbatim | `it_end_to_end::template_not_found_returns_404` | |
| POST on `/healthz` → 405 | Port with fixture edits | `it_end_to_end::healthz_post_is_method_not_allowed` | JS body: `{"error":"MethodNotAllowed"}`; Rust body adds `"message"`. D-014. |

## §D Cross-implementation reproduction plan (R4.3)

For every "Port verbatim" row above the target MUST also exist as a
cross-implementation fixture: same JSON body posted to both JS and
Rust, response diffed for equivalence modulo documented divergences.
See `compat/repro/` (landed in this refacto batch) and
`tests/repro_cross_impl.rs`.

## §E What this port plan does NOT cover

Per R3.4:

- **Load / concurrency tests** — neither impl ships them. Not planned
  for this refacto batch.
- **Docker container smoke** — covered by publish.yml, not by cargo
  test, not by this port plan.
- **CI matrix parity** — JS has no CI in the source repo; Rust has
  four workflows. Not comparable.
- **Handlebars-engine behavioural equivalence beyond what shipped
  templates exercise** — e.g., `{{#each}}` with sparse arrays,
  helpers not shipped in JS, block-helper argument coercion. Out of
  scope for a folder-drop template engine that trusts its input.
