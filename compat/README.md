# Compat corpus

**Spec**: `Buerostack/REFACTO-REQUIREMENTS.md §7.3`.

`js-DSL/` is a **verbatim copy** of `Buerostack/DataMapper/DSL/`
(the JS source of truth at `v1.0.0`). The Rust target's CI runs
every template here through the Rust renderer with the exact
JSON body that the template's inline curl comment recommends,
asserting the render succeeds (200).

Any parse or render failure without a corresponding entry in
[`../DIVERGENCES.md`](../DIVERGENCES.md) is a **build failure**.

Test harness: [`../tests/it_compat_js_dsl_corpus.rs`](../tests/it_compat_js_dsl_corpus.rs).

## Updating this corpus

When the JS source-of-truth ships new DSL samples:

```bash
rm -rf compat/js-DSL
cp -r ../DataMapper/DSL compat/js-DSL
```

Then re-run `cargo test --test it_compat_js_dsl_corpus`.
Any new failures either:
1. Reveal a source-compat regression → fix the Rust side, OR
2. Reveal a new intentional divergence → add to `DIVERGENCES.md`.

Never edit files under `js-DSL/`. If a divergence forces a
template rewrite, that goes under `../DSL/samples/`, not here.

## Cross-implementation reproduction

The R4.3 harness lives at
[`../tests/it_repro_cross_impl.rs`](../tests/it_repro_cross_impl.rs).
It requires the JS source repo to be checked out at
`../DataMapper/` (sibling to this repo). When absent, the tests
skip with a diagnostic — CI must set `DATAMAPPER_JS_SOURCE` to a
readable path to gate on repro parity.
