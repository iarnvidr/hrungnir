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
  --enable-static --disable-shared --disable-gpl --disable-nonfree --disable-autodetect --disable-doc --disable-debug --disable-ffplay --disable-ffprobe --enable-zlib --enable-securetransport --enable-videotoolbox --enable-audiotoolbox --enable-libdav1d --pkg-config-flags=--static --arch=arm64 --extra-cflags=-mmacosx-version-min=14.0 --extra-ldflags=-mmacosx-version-min=14.0
  ```

  Only Apple system frameworks (VideoToolbox, AudioToolbox, Secure Transport),
  zlib and libdav1d are linked. The binary is a separate program; the app does not
  link it.
  You may replace the bundled `Contents/Resources/bin/ffmpeg` with your own build
  of the same or a modified source.

## dav1d 1.5.4 (BSD-2-Clause), inside FFmpeg

AV1 software decoding. Built statically into the FFmpeg binary from the official
source release https://downloads.videolan.org/pub/videolan/dav1d/1.5.4/dav1d-1.5.4.tar.xz
(SHA-256 `686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5`,
the publisher's own digest), unmodified, by `scripts/ffmpeg_lgpl.py`. Its license
text, which must accompany binary redistribution, is `dav1d-COPYING`.

## yt-dlp 2026.08.19

The publisher's standalone macOS executable, from
https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19 , run by the app as a
separate program. The yt-dlp source is Unlicense (`yt-dlp-LICENSE`).

The executable bundles third-party components under other licenses, all listed
with their full texts in `yt-dlp-THIRD_PARTY_LICENSES.txt`. In the macOS build the
copyleft components are mutagen (GPL-2.0-or-later; the GPL v2 text is in that
file; source: https://github.com/quodlibet/mutagen) and certifi (MPL-2.0). The
file's other GPL and LGPL entries (GNU Readline, libstdc++, libgcc, libintl,
libidn2, libunistring) are marked there as Linux-only. Python and the remaining
components use permissive licenses; source for each is available from the
project URLs in the file, and yt-dlp's own source is at
https://github.com/yt-dlp/yt-dlp/tree/2026.08.19 . These GPL-licensed components
run as part of a separate program that Hrungnir starts as an independent process;
Hrungnir does not link them or include their code.

`scripts/fetch-media-tools.py` pins the yt-dlp binary and the FFmpeg source (and
`scripts/ffmpeg_lgpl.py` the dav1d source) by version and SHA-256 and rejects
mismatches. Helpers are bundled for macOS; Linux development uses separately
installed tools. The application does not install or update helpers at runtime,
and does not invoke a shell with page-supplied command strings.
