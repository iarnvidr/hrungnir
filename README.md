<p align="center">
  <img src="assets/hrungnir-logo.png" width="220" alt="Hrungnir, a giant carrying a spear">
</p>

<h1 align="center">Hrungnir</h1>
<p align="center">Reddit, ready to read offline.</p>

<p align="center">
  <a href="https://github.com/iarnvidr/hrungnir/actions/workflows/ci.yml"><img src="https://github.com/iarnvidr/hrungnir/actions/workflows/ci.yml/badge.svg?branch=main" alt="Linux CI"></a>
  <a href="https://github.com/iarnvidr/hrungnir/actions/workflows/macos.yml"><img src="https://github.com/iarnvidr/hrungnir/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS build"></a>
  <a href="https://github.com/iarnvidr/hrungnir/releases/latest"><img src="https://img.shields.io/github/v/release/iarnvidr/hrungnir" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="Apache 2.0 license"></a>
</p>

Hrungnir is an unofficial desktop Reddit reader for **Apple Silicon Macs running
macOS 14 or newer**. It keeps posts, comments and downloaded media locally, with a
compact reader built in Tauri, Rust and JavaScript.

## Download

Get the app ZIP and its SHA-256 file from the [latest release](https://github.com/iarnvidr/hrungnir/releases/latest).

1. Download both files into the same folder. Verify the ZIP before extracting it:

   ```sh
   shasum -a 256 -c Hrungnir-0.6.8-macos-arm64.zip.sha256
   ```

   The result should end in `OK`.
2. Extract the ZIP and move **Hrungnir.app** to Applications.
3. Open the app. Builds are ad-hoc signed and are **not notarized**; macOS may
   require approval in **System Settings → Privacy & Security**.

The app includes yt-dlp and an LGPL-only FFmpeg build with dav1d; no separate
media-tool installation is needed. Intel Macs and Windows are not supported by
the downloadable app. Linux is used for development and testing.

## What it does

- Read subreddit listings and threaded comments, with collapsed replies,
  parent previews and saved reading position.
- Download images and supported videos, including audio, for local playback.
- Search cached content immediately, or press Enter to search Reddit.
- Save posts and linked articles, images or videos into library collections;
  import bookmarks and export or restore your library.
- Choose light or dark themes, manage cache retention, and export diagnostics.
- Use a headless CLI against the same data as the desktop app.

Cached content remains available offline while retained. Cache budgets and
retention settings can remove older content; use **Keep forever** for saved
library items you want protected. See [Library](docs/LIBRARY.md) for details.

Data is stored in `~/Library/Application Support/io.github.iarnvidr.hrungnir/`.

## Build from source

On an Apple Silicon Mac, install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/),
Node.js 22, Python 3.12+, and Rust 1.90 or newer. Then:

```sh
git clone https://github.com/iarnvidr/hrungnir.git
cd hrungnir
brew install meson ninja pkgconf
npm ci
python3 scripts/fetch-media-tools.py
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --locked --manifest-path src-tauri/Cargo.toml \
  --release --target aarch64-apple-darwin --features custom-protocol
python3 scripts/package-macos.py
```

This creates `artifacts/Hrungnir.app`. Helper sources and downloads are pinned by
SHA-256; the first FFmpeg/dav1d build takes several minutes. Follow the
[release checklist](docs/RELEASE-CHECKLIST.md) for testing, signing and archiving.
The same build and artifact checks run on hosted macOS in GitHub Actions.

For portable core checks:

```sh
export TMPDIR="$(python3 -c 'import pathlib, tempfile; print(pathlib.Path(tempfile.gettempdir()).resolve())')"
cargo test --locked
cargo clippy --locked --all-targets
python3 scripts/test-macos-policy.py
python3 scripts/test-ffmpeg-lgpl.py
node --check reader/app.js
```

## Documentation and contributing

[macOS](docs/MACOS.md) · [CLI](docs/CLI.md) · [Library](docs/LIBRARY.md) ·
[App links](docs/APP_LINKS.md) · [Diagnostics](docs/SUPPORT.md) ·
[Architecture](docs/ARCHITECTURE.md) · [Roadmap](docs/PLAN.md) ·
[Contributing](CONTRIBUTING.md)

Report problems through [GitHub Issues](https://github.com/iarnvidr/hrungnir/issues).
Include the app version, macOS version and reproduction steps. Review diagnostic
exports before sharing them publicly.

Changes to `main` go through pull requests with passing Linux and macOS checks.

## License

Hrungnir is licensed under [Apache 2.0](LICENSE). Bundled executables have their
own licenses and notices in [`licenses/`](licenses/MEDIA-TOOLS.md), also included
inside the app bundle.
