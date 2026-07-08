#!/usr/bin/env bash
# Build the ferrox Web SDK (npm package) from the ferrox-web crate.
#
# Produces web/pkg/ containing:
#   - ferrox_web_bg.wasm        the compiled engine
#   - ferrox_web.js             ESM loader + bindings
#   - ferrox_web.d.ts           generated TypeScript types (the `Editor` class)
#   - package.json              ready to `npm publish` / `npm link`
#
# Requires wasm-pack:  cargo install wasm-pack   (or see https://rustwasm.github.io/wasm-pack)
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-bundler}"   # bundler | web | nodejs
echo "Building ferrox-web (wasm-pack --target $TARGET)…"

wasm-pack build web \
  --release \
  --target "$TARGET" \
  --out-dir pkg \
  --out-name ferrox_web

echo "Done → web/pkg/ (import { Editor } from 'ferrox-web')"
