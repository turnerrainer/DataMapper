# 003 — OpenTelemetry `traceparent` propagation

## Filed
2026-07-29 — PATTERNS.md §4 mandates W3C tracecontext at the
framework layer for every Buerostack core component.

## Severity
Medium. Not blocking anything today; blocks a Grafana-Tempo-based
audit correlation once the wider stack adopts the pattern.

## Motivation
DataMapper is on the request path for the majority of consumer
traffic in the stack. A missing span here means correlation breaks
at the shaping boundary — investigators lose the join between a UI
request and the upstream call that produced its raw data.

## Fix / Design
- Add `tracing-opentelemetry` + `opentelemetry-otlp` + resource
  detection deps.
- On startup, if `OTEL_EXPORTER_OTLP_ENDPOINT` is set, initialise
  an OTLP exporter; otherwise fall through to the current
  `tracing_subscriber::fmt` layer.
- On request ingest: read `traceparent` header; adopt if present,
  otherwise generate a new trace + span.
- Echo `traceparent` + emit `X-Trace-Id` on every response.
- Wrap the render path in a span with `dsl.project` +
  `dsl.view` attributes.

## Acceptance
- [ ] Request with valid `traceparent` → same trace id appears in
      exported span.
- [ ] Request without `traceparent` → new trace id generated,
      valid W3C shape.
- [ ] Response echoes `traceparent` + `X-Trace-Id`.
- [ ] Integration test spins up a mock OTLP collector, asserts
      span export.
- [ ] Book adds `book/src/observability.md`; introduction
      references it.

## Estimated effort
1–2 days.

## Dependencies
- 002 (MVP)

## Non-scope
- Metrics export — separate task.
- Log export via OTLP — leave to `RUST_LOG` + stdout for now.

## Risks
- OTLP exporter timing out on missing endpoint could delay
  startup. Mitigation: `try_from_default_env` + timeout-bounded
  init, log-and-continue if the collector is unreachable.
