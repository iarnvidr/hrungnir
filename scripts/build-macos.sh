#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
product_name="$(python3 -c 'import json; print(json.load(open("src-tauri/tauri.conf.json"))["productName"])')"
# Cross-build from Linux in a disposable container, without mounts, profiles or
# signing credentials. The image must provide python3, cargo-zigbuild, the
# aarch64-apple-darwin Rust target, a macOS SDK at /opt/macos-sdk/current and
# rcodesign. On a Mac, build natively instead (see README.md).
build_image="${REDDIT_MACOS_BUILD_IMAGE:?Set REDDIT_MACOS_BUILD_IMAGE to a macOS cross-build container image}"
build_container="reddit-offline-macos-$$"
trap 'docker rm -f "$build_container" >/dev/null 2>&1 || true' EXIT
docker run -d --name "$build_container" --entrypoint sleep "$build_image" infinity >/dev/null
docker exec "$build_container" mkdir -p /work
tar --exclude=target --exclude=node_modules --exclude=var --exclude=artifacts --exclude=output -cf - . |
  docker exec -i "$build_container" tar -xf - -C /work
docker exec -w /work "$build_container" python3 scripts/fetch-media-tools.py
docker exec -w /work -e SDKROOT=/opt/macos-sdk/current -e MACOSX_DEPLOYMENT_TARGET=14.0 "$build_container" \
  cargo zigbuild --locked --manifest-path src-tauri/Cargo.toml --release --target aarch64-apple-darwin --features custom-protocol
docker exec -w /work "$build_container" python3 scripts/package-macos.py
docker exec -w /work "$build_container" rcodesign sign "artifacts/${product_name}.app/Contents/Resources/bin/yt-dlp"
docker exec -w /work "$build_container" rcodesign sign "artifacts/${product_name}.app/Contents/Resources/bin/ffmpeg"
docker exec -w /work "$build_container" rcodesign sign "artifacts/${product_name}.app"
mkdir -p artifacts
docker cp "$build_container:/work/artifacts/${product_name}.app" artifacts/
python3 scripts/verify-macos.py
python3 scripts/archive-macos.py
