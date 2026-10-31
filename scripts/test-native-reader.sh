#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(dirname -- "$script_dir")
toolchain_bin=$(dirname -- "$(rustup which cargo)")
cd "$repo_root"
PATH="$toolchain_bin:$PATH"
export PATH
MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-14.0}
export MACOSX_DEPLOYMENT_TARGET
# Use one known build location and the dedicated test app identity.
CARGO_TARGET_DIR="$repo_root/src-tauri/target"
export CARGO_TARGET_DIR
mkdir -p "$repo_root/var"
test_root=$(mktemp -d "$repo_root/var/native-reader.XXXXXX")
cargo tauri build --debug --no-bundle --config src-tauri/tauri.test.conf.json
HRUNGNIR_DATA_DIR="$test_root/data"
HRUNGNIR_NATIVE_PROBE="$test_root/result.json"
HRUNGNIR_NATIVE_PROBE_MODE=ui
HRUNGNIR_NATIVE_UI_REQUIRE_PAGING=1
export HRUNGNIR_DATA_DIR HRUNGNIR_NATIVE_PROBE HRUNGNIR_NATIVE_PROBE_MODE HRUNGNIR_NATIVE_UI_REQUIRE_PAGING
printf 'Native reader report: %s\n' "$HRUNGNIR_NATIVE_PROBE"
exec "$CARGO_TARGET_DIR/debug/offline-reader-desktop"
