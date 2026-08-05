# Compat corpus

`js-DSL/` is a **verbatim copy** of `Buerostack/DataMapper/DSL/`
(the JS source of truth at `v1.0.0`). The Rust target's CI runs
every template here through the Rust renderer with the exact
JSON body that the template's inline curl comment recommends,
asserting the render succeeds (200).

Any parse or render failure without a corresponding note in
[`book/src/porting-from-js.md`](../book/src/porting-from-js.md)
is a **build failure**.

Test harness: [`../tests/it_compat_js_dsl_corpus.rs`](../tests/it_compat_js_dsl_corpus.rs).

## Updating this corpus

When the JS source-of-truth ships new DSL samples:

```bash
rm -rf compat/js-DSL
cp -r ../DataMapper/DSL compat/js-DSL
```

Then re-run `cargo test --test it_compat_js_dsl_corpus`. Any new
failure either:
1. Reveals a source-compat regression → fix the Rust side, OR
2. Reveals a new intentional divergence → update
   `book/src/porting-from-js.md`.

Never edit files under `js-DSL/`. If a divergence forces a
template rewrite, that goes under `../DSL/samples/`, not here.

## Cross-implementation reproduction

`../tests/it_repro_cross_impl.rs` posts identical bodies to both
the JS server (staged under `js-server/`) and the Rust binary, and
diffs the responses. Set it up once:

```bash
./scripts/setup-repro.sh
```

Then run `cargo test --test it_repro_cross_impl`. The test skips
loudly if the JS server isn't staged; set
`DATAMAPPER_REPRO_STRICT=1` in CI to gate on it.
