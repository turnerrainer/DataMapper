# 005 — Helper library expansion (uppercase, lowercase, date fmt)

## Filed
2026-07-29 — surfaced during sample DSL port. The Node.js
DataMapper only shipped `now` + `json`, but the JS Handlebars
runtime supported ad-hoc helpers registered via a project's own
`helpers.js`. Rust needs a fixed helper set.

## Severity
Low. Adds convenience; no consumer is blocked.

## Motivation
Common template needs today:
- `{{uppercase name}}`, `{{lowercase code}}`
- `{{fmt_date created_at "%Y-%m-%d"}}` (chrono strftime)
- `{{default value fallback}}` (like Handlebars' `default` but
  without the DSL author writing `{{#if}}/{{else}}` every time)

## Fix / Design
- Land as one PR per helper family. Every helper gets:
  - unit test in `helpers.rs`
  - integration test that hits it via a real HTTP round-trip
  - one paragraph + example in `configuration.md`.

## Acceptance
- [ ] `uppercase`, `lowercase`, `default`, `fmt_date` implemented.
- [ ] Book "Built-in helpers" section covers each.
- [ ] Tests: unit + integration for each.

## Estimated effort
0.5 days per helper family.

## Dependencies
- 002 (MVP)

## Non-scope
- User-plugin helpers (loading `.wasm` or shared libs at runtime
  is an admission of dynamic code execution and violates
  DESIGN.md §2 / DEV-REQUIREMENTS §5.3).

## Risks
- Feature creep. Mitigation: every helper must have a
  before-adding test case with a concrete DSL that needs it —
  no speculative additions.
