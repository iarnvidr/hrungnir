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

exec cargo tauri dev --config src-tauri/tauri.dev.conf.json "$@"
