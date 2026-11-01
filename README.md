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

- Browse subreddit listings as they load, and read threaded comments with
  collapsed replies, parent previews and saved reading position.
- Download images and supported videos, including audio, for local playback.
- Search all Reddit, the current community, or downloaded posts and comments;
  episode searches match common alternative numbering formats.
- Navigate posts, subreddits and comment trees with hjkl; use `/` to search and
  `?` for shortcuts. See [Search and keyboard navigation](docs/KEYBOARD-SEARCH.md).
- Save posts and linked articles, images or videos into library collections;
  import bookmarks and export or restore your library.
- Choose light or dark themes, manage cache retention, and export diagnostics.
- Use a headless CLI against the same data as the desktop app.

Cached content remains available offline while retained. Cache budgets and
retention settings can remove older content; use **Keep forever** for saved
library items you want protected. See [Library](docs/LIBRARY.md) for details.

Data is stored in `~/Library/Application Support/io.github.iarnvidr.hrungnir/`.

## Development and builds

Push source changes to a pull request. GitHub Actions compiles the app and Rust
tests, runs the container browser checks, and produces the macOS archives.
An explicit main-branch build can be requested with
`gh workflow run macos.yml --ref main`. Release packaging follows the
[release checklist](docs/RELEASE-CHECKLIST.md).

Builds and compile-based checks run only in Actions. The old local development
and cross-build entrypoints refuse local compilation. Node and ChromeDriver
are not used for development or testing.

For a completed macOS Actions run, download, verify and run its dedicated
hidden native test app with:

```sh
sh scripts/test-native-reader.sh RUN_ID
```

This verifies the source commit, artifact receipt, archive hash, test identity
and signature before applying the one-launch-per-30-minutes gate. It uses
`src-tauri/tauri.test.conf.json` and fresh isolated data; it does not build
locally or use the installed app's profile.

Existing portable CLI binaries and non-compiling Python/shell checks may run
locally. See [macOS verification](docs/MACOS.md) for details.

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
