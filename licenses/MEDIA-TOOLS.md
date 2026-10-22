# Bundled media helpers

The app invokes these independent executables as child processes. No browser
cookies, browser profiles or account credentials are passed to them.

- yt-dlp 2026.08.19: official standalone macOS universal executable from
  https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19 . Source and third-party
  licenses are included as `yt-dlp-LICENSE` and `yt-dlp-THIRD_PARTY_LICENSES.txt`.
- FFmpeg 9.0.2: static Apple Silicon GPL build published by Martin Riedl at
  https://ffmpeg.martin-riedl.de/ . The GPL text is included as
  `ffmpeg-COPYING.GPLv3`. Source/build scripts and dependency acquisition recipes:
  https://git.martin-riedl.de/ffmpeg/build-script . FFmpeg upstream source:
  https://git.ffmpeg.org/ffmpeg.git/tree/n9.0.2 . These tools retain their own
  licenses; their source is available through the linked publishers.

`scripts/fetch-media-tools.py` pins publisher artifacts and SHA-256 hashes. It
rejects mismatches. Helpers are bundled for macOS; Linux development uses
separately installed tools. The application does not install or update helpers
at runtime, and does not invoke a shell with page-supplied command strings.
