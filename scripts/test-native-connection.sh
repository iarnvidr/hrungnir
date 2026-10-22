#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  printf 'Usage: %s /path/to/offline-reader-desktop /path/to/new-output.zip\n' "$0" >&2
  exit 2
fi

APP_BIN=$1
OUTPUT_ZIP=$2

if [[ ! -f "$APP_BIN" || ! -x "$APP_BIN" ]]; then
  printf 'App executable is missing or not executable: %s\n' "$APP_BIN" >&2
  exit 2
fi
if [[ "$OUTPUT_ZIP" != *.zip ]]; then
  printf 'Output path must end in .zip: %s\n' "$OUTPUT_ZIP" >&2
  exit 2
fi
if [[ -e "$OUTPUT_ZIP" ]]; then
  printf 'Output ZIP already exists; choose a new path: %s\n' "$OUTPUT_ZIP" >&2
  exit 2
fi

HOST_OS=$(uname -s)
case "$HOST_OS" in
  Darwin)
    printf 'Running the connection test natively on macOS.\n'
    ;;
  Linux)
    printf 'Running the connection test natively on Linux; this does not verify macOS behavior.\n'
    ;;
  *)
    printf 'Unsupported native test host: %s\n' "$HOST_OS" >&2
    exit 2
    ;;
esac

OUTPUT_PARENT=$(dirname "$OUTPUT_ZIP")
mkdir -p "$OUTPUT_PARENT"
exec "$APP_BIN" --connection-test "$OUTPUT_ZIP"
