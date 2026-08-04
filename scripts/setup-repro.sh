#!/usr/bin/env bash
#
# Bootstrap the JS side of the cross-impl repro harness. Idempotent.
#
# Downloads the express + express-handlebars deps declared in the
# staged `compat/js-server/package.json` so `tests/it_repro_cross_impl.rs`
# can boot a real JS DataMapper alongside the Rust build.
#
# Run once per fresh checkout, or after `git clean -x compat/`.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
JS_DIR="$REPO/compat/js-server"

if [ ! -f "$JS_DIR/package.json" ]; then
  echo "!! $JS_DIR/package.json missing — the refacto pass was supposed to stage it" >&2
  exit 1
fi

command -v npm >/dev/null 2>&1 || {
  echo "!! npm not installed — required to install the JS side" >&2
  exit 1
}

pushd "$JS_DIR" >/dev/null
if [ ! -d node_modules ]; then
  echo ">> installing JS deps into $JS_DIR/node_modules ..."
  npm install --omit=dev --silent
else
  echo ">> $JS_DIR/node_modules already present — skipping install"
fi
popd >/dev/null

echo ">> ready. Run: cargo test --test it_repro_cross_impl"
