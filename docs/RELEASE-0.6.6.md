# Hrungnir 0.6.6

Fixes premature recent-media eviction: new files are staged successfully before eviction, and only their actual completed size is counted. Cache reuse refreshes LRU recency without resetting watched/unwatched retention. Failed and empty downloads preserve existing media.

Listing refreshes preserve complete cached post details, including discussions with zero comments. Equivalent Reddit HLS/DASH URLs reuse the same local video; missing generated posters no longer force video downloads.

The reader now reports **Opened from cache** when native post/media caches are reused. Support logs include cache miss reasons and byte-counted budget eviction events. The 512 MiB unprotected-media limit remains; real space pressure can still evict files sooner. Keep forever and active playback remain protected.

## First public release

- Apple Silicon app for macOS 14+, using the bundle identifier `io.github.iarnvidr.hrungnir`.
- Apache 2.0 application license, with bundled third-party license notices.
- Pinned yt-dlp and a source-built LGPL-only FFmpeg 9.0.2 with dav1d 1.5.4.
- Ad-hoc signed app ZIP and an accompanying SHA-256 checksum. The app is not notarized; macOS may require approval in Privacy & Security.

Download both release files into one folder, then run:

```sh
shasum -a 256 -c Hrungnir-0.6.6-macos-arm64.zip.sha256
```

Expect `OK`. Extract the ZIP and move Hrungnir.app to Applications.

## Verification

Hosted Linux and native Apple Silicon macOS CI pass all 153 core tests (one explicit network probe remains ignored). Native macOS checks also build the helpers, encode H.264/AAC, merge separate streams through yt-dlp, decode the result, generate a JPEG poster, compile the Tauri app, verify the signed and extracted ZIP, and execute its packaged CLI. Interactive GUI and live-service behavior are separate from these automated checks.
