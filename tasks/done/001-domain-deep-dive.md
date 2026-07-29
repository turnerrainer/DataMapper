# 001 — Domain deep-dive

## Filed
2026-07-29 — DEV-REQUIREMENTS.md §10 requires every new
`<Product>-on-Rust` project to start with a domain design task.

## Landed
2026-07-29 — commit `<sha>`. Landed as `docs/DESIGN.md`, defining
purpose, non-goals, data flow, route model, output negotiation,
per-request template read policy, path-traversal defence, response
cap, `len` helper, and v0.1 "done" criteria.

## Severity
High. Every subsequent task references the design; ambiguity here
propagates.

## Motivation
The Node.js DataMapper had no committed design doc — behaviour was
"what the code does." The rewrite is a chance to write down the
intended behaviour so DSL authors, operators, and future
contributors can rely on it rather than reverse-engineering it.

## Fix / Design
Write `docs/DESIGN.md` covering:
- Purpose and non-goals
- Data flow diagram
- Route model + template lookup order
- Output negotiation (JSON vs HTML, header precedence)
- Non-strict Handlebars decision (compat with JS Handlebars)
- Path traversal defence layers
- No admin HTTP surface (DEV-REQUIREMENTS §5.3)
- Response body cap rationale
- Custom `len` helper rationale
- Config surface
- Failure mode catalogue location
- v0.1 acceptance + v0.2 speculation

## Acceptance
- [x] `docs/DESIGN.md` present.
- [x] Zero-knowledge reader can answer "what is DataMapper for?"
- [x] Every behaviour decision has a stated reason.
- [x] Explicit non-goals section.

## Estimated effort
0.5 days.

## Dependencies
None.

## Non-scope
- Rewriting the Node.js DataMapper behaviour docs.
- Deciding v0.2 features (only listing candidates).

## Risks
- Design doc drifts from code as features land. Mitigation: every
  behaviour-changing PR updates the relevant DESIGN.md section in
  the same commit.
