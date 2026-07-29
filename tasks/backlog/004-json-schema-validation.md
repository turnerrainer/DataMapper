# 004 — Optional per-DSL JSON schema validation

## Filed
2026-07-29 — surfaced during MVP design review. Not required for
v0.1 but a natural extension.

## Severity
Medium. Consumers today can send garbage request bodies and get
opaque render errors; a schema would produce a structured
`InvalidRequest` at the boundary.

## Motivation
Templates fail cryptically when the client sends a body of the
wrong shape (`{{user.address.street}}` where `user` is missing).
An optional JSON schema per DSL would produce a clear 422 with a
JSON-Pointer to the offending field, before the renderer runs.

## Fix / Design
- Convention: a template at `<project>/<view>.hbs` may have a
  sibling `<view>.schema.json`. If present, the request body is
  validated against it before rendering.
- Use `jsonschema` crate (or `boon`).
- Validation failure → 422 + JSON body pointing at the failing
  field.
- Schema-less templates behave unchanged (opt-in).

## Acceptance
- [ ] `write_dsl_with_schema` integration test helper.
- [ ] Test: schema-passing body renders.
- [ ] Test: schema-failing body → 422 with JSON-Pointer.
- [ ] Test: template with no schema renders unchanged.
- [ ] Configuration.md documents the sibling-file convention.

## Estimated effort
1–2 days.

## Dependencies
- 002 (MVP)

## Non-scope
- Response schema validation (out — DataMapper does not own the
  response contract; the DSL author does).
- Schema hot-reload — same policy as templates (read per request).

## Risks
- Adds a per-request read for the schema. Mitigation: cache
  schema compilation, keyed by `<file, mtime>`.
