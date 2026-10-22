# Hrungnir

Offline Reddit client: a local, cache-first reader for Apple Silicon Macs, built with Tauri, Rust and JavaScript.

Posts, comments and media are downloaded to local storage before they are shown, so anything you have opened stays readable offline. A headless CLI (`--cli`) shares the same cache.

## Platform notes

- **macOS 14+ on Apple Silicon** is the supported platform for the packaged app.
- Linux builds are for development and testing only; they need yt-dlp and FFmpeg on `PATH`.
- Release builds are ad-hoc signed, not notarized. macOS may ask you to approve the app in Privacy & Security. See [docs/MACOS.md](docs/MACOS.md).

## Build from source

Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), Node.js and a Rust toolchain (1.85+).

```sh
npm ci
cargo test --locked      # core tests (no network needed)
npm run dev              # run the app in development mode
```

Apple Silicon app bundle, on a Mac:

```sh
npm ci
python3 scripts/fetch-media-tools.py                       # pinned, checksum-verified yt-dlp and FFmpeg
cargo build --locked --manifest-path src-tauri/Cargo.toml \
  --release --target aarch64-apple-darwin --features custom-protocol
python3 scripts/package-macos.py                           # assembles artifacts/Hrungnir.app
```

`scripts/build-macos.sh` does the same as a Linux cross-build inside a container image you provide (`REDDIT_MACOS_BUILD_IMAGE`).

## Documentation

[Architecture](docs/ARCHITECTURE.md) · [macOS build and run](docs/MACOS.md) · [CLI](docs/CLI.md) · [Library](docs/LIBRARY.md) · [App links](docs/APP_LINKS.md) · [Support diagnostics](docs/SUPPORT.md) · [UX](docs/UX.md) · [Next steps](docs/PLAN.md) · [Contributing](CONTRIBUTING.md)

Third-party notices for bundled components are in [`licenses/`](licenses/).

## License

Licensed under the Apache License, Version 2.0 (see [LICENSE](LICENSE)).
