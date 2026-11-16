#!/usr/bin/env bash
# Lint, test and the coverage gate for the macOS app. KARAMD_BIN adds the test against the real CLI.
set -euo pipefail
cd "$(dirname "$0")/.."

xcrun swift-format lint --strict -r -p Package.swift Sources Tests App
swift test --enable-code-coverage -Xswiftc -warnings-as-errors
scripts/coverage.rb
