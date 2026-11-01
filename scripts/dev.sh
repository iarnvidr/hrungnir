#!/bin/sh
set -eu

if [ "${GITHUB_ACTIONS:-}" != "true" ]; then
  printf '%s\n' 'Local compilation is disabled. Push a PR for GitHub Actions, then use sh scripts/test-native-reader.sh RUN_ID for the verified native test artifact.' >&2
  exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(dirname -- "$script_dir")
toolchain_bin=$(dirname -- "$(rustup which cargo)")

cd "$repo_root"
PATH="$toolchain_bin:$PATH"
export PATH
MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-14.0}
export MACOSX_DEPLOYMENT_TARGET

exec cargo tauri dev --config src-tauri/tauri.dev.conf.json "$@"
