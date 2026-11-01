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
TMPDIR=$(python3 -c 'import pathlib,tempfile; print(pathlib.Path(tempfile.gettempdir()).resolve())')
export TMPDIR
mkdir -p "$repo_root/var"
test_root=$(mktemp -d "$repo_root/var/native-reader.XXXXXX")
cargo test --locked --manifest-path Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
python3 scripts/test-macos-policy.py
cargo tauri build --debug --no-bundle --config src-tauri/tauri.test.conf.json
python3 scripts/native-test-window.py --claim
HRUNGNIR_DATA_DIR="$test_root/data"
HRUNGNIR_NATIVE_PROBE="$test_root/result.json"
HRUNGNIR_NATIVE_PROBE_MODE=reader
HRUNGNIR_NATIVE_UI_REQUIRE_PAGING=1
export HRUNGNIR_DATA_DIR HRUNGNIR_NATIVE_PROBE HRUNGNIR_NATIVE_PROBE_MODE HRUNGNIR_NATIVE_UI_REQUIRE_PAGING
printf 'Native reader report: %s\n' "$HRUNGNIR_NATIVE_PROBE"
native_exit=0
"$CARGO_TARGET_DIR/debug/offline-reader-desktop" || native_exit=$?
python3 scripts/native-reader-report.py "$HRUNGNIR_NATIVE_PROBE"
exit "$native_exit"
