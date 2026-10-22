# Bundled media helpers

The app invokes these independent executables as child processes. No browser
cookies, browser profiles or account credentials are passed to them.

## FFmpeg 9.0.2 (LGPL 2.1 or later)

Built from the official FFmpeg source release, with no GPL and no non-free
components (no `--enable-gpl`, `--enable-nonfree`, `--enable-version3`, x264 or x265).
The license text is `ffmpeg-COPYING.LGPLv2.1`.

- Source: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
  (SHA-256 `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`;
  signature https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz.asc). The source is
  unmodified.
- Build recipe: `scripts/ffmpeg_lgpl.py`, run by `scripts/fetch-media-tools.py` on an
  Apple Silicon Mac. It verifies the tarball digest, builds, and then checks that
  the result reports the LGPL.
- Configure options (the string `ffmpeg -version` prints after `configuration:`;
  the build also writes it to `var/media-tools/ffmpeg.configuration.txt`):

  ```
  --enable-static --disable-shared --disable-gpl --disable-nonfree --disable-autodetect --disable-doc --disable-debug --disable-ffplay --disable-ffprobe --enable-zlib --enable-securetransport --enable-videotoolbox --enable-audiotoolbox --arch=arm64 --extra-cflags=-mmacosx-version-min=14.0 --extra-ldflags=-mmacosx-version-min=14.0
  ```

  Only Apple system frameworks (VideoToolbox, AudioToolbox, Secure Transport)
  and zlib are linked. The binary is a separate program; the app does not link it.
  You may replace the bundled `Contents/Resources/bin/ffmpeg` with your own build
  of the same or a modified source.

## yt-dlp 2026.08.19

The publisher's standalone macOS executable, from
https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19 . The yt-dlp source is
Unlicense (`yt-dlp-LICENSE`). The executable also contains third-party components
under other licenses, including GPL-3.0-or-later; see
`yt-dlp-THIRD_PARTY_LICENSES.txt`.

`scripts/fetch-media-tools.py` pins both helpers by version and SHA-256 and rejects
mismatches. Helpers are bundled for macOS; Linux development uses separately
installed tools. The application does not install or update helpers at runtime,
and does not invoke a shell with page-supplied command strings.
