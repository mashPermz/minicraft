#!/bin/sh
# ビルドして http://localhost:8080 で配信する
set -e
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/minicraft.wasm web/
echo "open http://localhost:8080"
cd web && exec python3 -m http.server 8080
