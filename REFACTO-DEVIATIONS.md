# REFACTO-DEVIATIONS — DataMapper-on-Rust

**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md §10.2`.
**Written**: 2026-08-04.
**Applies to**: any MUST-level requirement in
`REFACTO-REQUIREMENTS.md v1.0` that this project is intentionally not
meeting in the current pass. Silent opt-outs are FORBIDDEN — every
gap lives here with a name and a plan.

Deviation ID convention: `R{section}.{req}—{short-name}`.

---

## R2.3 — Default drift on `max_request_bytes` (documented, not fixed)

- **Requirement**: For every scalar field the source defaults, the
  target's default MUST match; a change requires §1.2 rationale + §5
  divergence entry + startup INFO on first boot.
- **Deviation**: Rust's default is 2 MiB binary (2 097 152); JS was
  `"2mb"` = 2 000 000 decimal. Delta is ~5%. See `DIVERGENCES.md#D-002`.
- **Startup log line**: not currently emitted; §6.2 diagnostic pass
  only calls out unwired fields. Adding a one-line INFO on first
  boot (when the operator's `max_request_bytes` matches the Rust
  default) would be theatrical. **Decision**: hold; if a real
  operator hits a payload in the 2 000 000–2 097 152 window we'll
  revisit.
- **Resolution plan**: v0.2 amendment — either flip the default to
  2 000 000 (JS-exact) or emit a §6.2 INFO. Track in `tasks/backlog/`.

## R2.5 — Timestamp format from `{{now}}` (documented, not fixed)

- **Requirement**: user-facing strings the source emits SHOULD have
  a target equivalent (same message or same underlying condition).
- **Deviation**: `chrono::to_rfc3339()` emits `+00:00` and higher
  precision; JS `.toISOString()` emits `Z` and ms precision. See
  `DIVERGENCES.md#D-008`.
- **Resolution plan**: v0.2 — swap to explicit format string
  `%Y-%m-%dT%H:%M:%S%.3fZ`. Deferred to avoid churn on the alpha
  release when no downstream consumer has been observed relying on
  the exact form.

## R3.2 — Cross-impl reproduction level for the `{{now}}` fixture

- **Requirement**: any finding acted on MUST be `(reproduced)`; a
  test that fails before and passes after must exist.
- **Deviation**: `{{now}}` is documented as divergent (D-008), not
  fixed, so we did not write a `(reproduced)` fixture for it — the
  cross-impl repro test in `tests/it_repro_cross_impl.rs` treats
  `ts` as a volatile key and strips it before comparing.
- **Resolution plan**: when D-008 is closed (per R2.5 deviation
  above), the `ts` field lands in the equivalence set.

## R4.3 — Cross-impl repro coverage is `type: json`-only

- **Requirement**: for every claimed-preserved subsystem, at least
  one end-to-end fixture must run against BOTH source and target
  and produce byte-identical output modulo documented divergences.
- **Deviation met**: yes for the `type: json` code path — every
  shipped sample DSL has a corresponding row in
  `tests/it_repro_cross_impl.rs`.
- **Sub-deviation**: HTML fallback path (`text/html; charset=utf-8`
  vs `text/html`) is compared by MIME family, not byte-equal.
  Documented D-012.
- **Sub-deviation**: opportunistic-JSON code path is exercised by
  the corpus in `type: json` mode; the `no-preference` branch of
  `respond()` is covered by `it_end_to_end::opportunistic_json_when_output_looks_like_json`
  but not against the JS impl. Deferred to v0.2.

## R6.3 — Config-field removal grace window

- **Requirement**: two-release grace window (accept + INFO → accept +
  WARN → reject) for removing an old field.
- **Deviation**: N/A in this pass — no field is being removed.
  `PORT` env var is being *added* (D-001). The `dsl_path` /
  `port` / `limits.*` are new (no JS predecessor).
- **Resolution plan**: N/A.

## R7.2 — `MIGRATION.md` structure

- **Requirement**: before/after DSL sample per divergence, indexed
  to `DIVERGENCES.md`.
- **Deviation met**: yes for `.length` (the only DSL-format
  divergence). See `MIGRATION.md#.length-accessor`. Other
  divergences are HTTP-shape or container-layout, covered elsewhere
  in `MIGRATION.md`.

## R7.3 — CI wiring for `compat/js-DSL`

- **Requirement**: syntactic corpus of source-of-truth files runs
  through the target parser on CI; any parse failure without a
  divergence entry is a build failure.
- **Deviation met**: harness exists (`tests/it_compat_js_dsl_corpus.rs`)
  and passes locally. **Sub-deviation**: CI wiring in
  `.github/workflows/tests.yml` for the compat corpus is not
  explicitly gated — the test runs as part of `cargo test`, which
  IS the CI step, so it fires implicitly. This satisfies the
  requirement but is worth documenting so a future CI split
  (e.g., extracting the compat run to a separate job) preserves
  the gate.

## R8.4 — Explicit "not covered" section on the audit

- **Requirement**: audit closes with a "what this audit did NOT
  cover" section.
- **Deviation met**: yes — `docs/REFACTO-AUDIT-S2.md` closes with
  "What this audit did NOT cover" per R3.4/R8.4.

## R9.1 — Stable-release gates

- **Requirement**: `MATCH` or `DIVERGE-documented` on every matrix
  row; cross-impl fixtures pass in CI; DIVERGENCES.md reviewed;
  boot diagnostic captured in release notes.
- **Deviation**: `v0.1.0-alpha.1` is a pre-release and lives under
  §9.2, not §9.1. When the target promotes to stable, we re-run
  this checklist and gate the tag on it.

## R9.2 — Pre-release known-gaps listing

- **Requirement**: pre-release MUST list its known coverage gaps in
  the release notes.
- **Deviation until this pass**: `v0.1.0-alpha.1`'s release notes
  claimed "targeting DEV-REQUIREMENTS compliance" without listing
  REFACTO gaps. Closed by amended `CHANGELOG.md` entry for this
  refacto batch (see task #14).

---

## What happens next

Each open deviation above carries a `Resolution plan` line. When the
resolution ships, remove the entry here (per R10.2) and confirm the
corresponding `REFACTO-REQUIREMENTS.md` MUST is now met.

Nothing else in `REFACTO-REQUIREMENTS.md` is being intentionally
skipped. If a future audit finds a silent gap not listed here, that's
a §10.2 violation and MUST result in either a fix or an appended
entry.
