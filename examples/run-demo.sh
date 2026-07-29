#!/usr/bin/env bash
# Quick demo: builds archscan (if needed) and runs it against a sample
# project that contains an intentional circular dependency
# (user.ts -> order.ts -> invoice.ts -> user.ts) plus one clean file.
set -e
cd "$(dirname "$0")/.."
if [ ! -f target/release/archscan ]; then
  echo "==> Building archscan (release mode, first run only, ~2-3 min)..."
  cargo build --release
else
  echo "==> archscan already built, skipping compile step."
fi
echo
echo "==> Running archscan on examples/circular-demo ..."
echo
./target/release/archscan analyze examples/circular-demo
