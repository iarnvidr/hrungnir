# Apple Silicon build and acceptance

Version 0.6.6 requires **macOS 14+ on Apple Silicon** for the native per-view proxy API. Quit the previous version before replacing **Hrungnir.app**, then enter `/r/soccer` in the subreddit panel and fetch posts. The header shows cached matches while typing; press Enter for live Reddit results. Click a post; existing content is reused, with missing comments/media downloaded before the local reader displays it. External article links offer inline text or a separate native webview. The bundle identifier is `io.github.iarnvidr.hrungnir`. On first launch after updating from an earlier build, its data (cache, media, webview storage, preferences, saved window state) is moved to the matching new-identifier locations; nothing is overwritten if new data already exists.

Install in Applications to register `hrungnir://open?url=…` links. Direct delivery also uses `open -a Hrungnir 'https://www.reddit.com/r/soccer/'`; see [app links](APP_LINKS.md). Native Mac Launch Services, cold/running delivery and window focus remain acceptance items; Linux tests and packaged scheme metadata do not prove those Mac behaviors.

The app bundles its reader, Rust/SQLite core, yt-dlp and FFmpeg. No Node, Python, Rust or Homebrew installation is needed to run the Mac app. Local media files supply playback. Site/download failures are shown with Retry and preserve the previous reader content.

## Build verification versus native Mac verification

The app is cross-compiled from Linux, assembled as a native ARM bundle and ad-hoc signed. Structural checks cover main executable architecture/system dependencies and code/resource hashes; media helpers are independently signed before the bundle. These checks do not prove native Mac launch, WKWebView behavior, Apple's codesign assessment or Gatekeeper acceptance.

The user's 0.5.2 Mac screenshot and support ZIP now establish launch, local reading and successful proxy acquisition after using certificate recovery: three completed TLS/HTTP captures, including a 943 ms listing. They also exposed the article-thumbnail media failure corrected in 0.5.3. This is user-provided native evidence, not an automated Mac test. Native verification of the revised 0.5.4 search, settings and video playback remains pending.

There is no Developer ID signature or notarization. macOS may require approval in Privacy & Security. On the destination Mac:

```sh
shasum -a 256 Hrungnir-0.6.6-macos-arm64.zip
codesign --verify --deep --strict --verbose=2 'Hrungnir.app'
open 'Hrungnir.app'
```

Compare with the adjacent checksum asset. Native Mac acceptance remains pending for this revision.

The acquisition proxy terminates TLS for public www Reddit HTML. Its CA is generated inside the app data directory. The primary trust path is the acquisition webview's exact-host delegate. If macOS still rejects the connection, Diagnostics offers **Allow proxy certificate…**. After explicit native confirmation, this adds only Hrungnir's public CA to the login Keychain, with a user-level SSL policy for `www.reddit.com`. Other apps in the same user account may also use that host trust. It does not install system/admin trust or read credentials; no certificate is installed automatically. Fetch posts again after installation. This follows Muninn's recovery approach with an additional hostname restriction.

0.5.1 refreshed WebKit's delegate capability cache and added the exact-host ATS exception for manual server-trust evaluation. User diagnostics subsequently showed delegate acceptance followed by EOF, which triggered premature worker destruction. In 0.5.2 an EOF/reset remains retryable within the existing 25-second acquisition deadline; explicit TLS alerts still fail promptly. EOF alone does not prove a certificate rejection. Logs distinguish retries, trust decisions and actual TLS successes. If acquisition fails, export a fresh support ZIP.

For an executable connection check, quit Hrungnir and run:

```sh
'/Applications/Hrungnir.app/Contents/MacOS/offline-reader-desktop' --connection-test "$HOME/Desktop/Hrungnir-native-check.zip"
```

Choose a new ZIP filename. This fetches a soccer listing and one comments page through two native workers, checks actual proxy capture, exports results and exits. It isolates test content/logs while retaining Hrungnir's own CA and app webview store. It does not install certificates or download media. A successful cross-build or Linux run does not establish this check passes on macOS.

## Native acceptance

1. Fetch `/r/soccer`; only the app's local classic-style reader should be visible.
2. Select an image post: wait for comments and a local image, without opening Reddit separately.
3. Select a supported video post: wait for download, play and seek the local file, and check audio.
4. Quit/reopen with networking disabled: the selected cached post/comments/media should still open and play.
5. Reconnect, change subreddit, try Retry after a download failure, resize, collapse replies and check parent previews/reading position.
6. Confirm no ads, cookie overlays, Live/Saved switches or “open live to get comments” notices appear.
7. Open Settings, switch System/Classified Dark/Classified Light and accents, then reopen to confirm appearance persistence. Check the media presentation preference there. Open Diagnostics and export a support ZIP; check that the native save dialog writes a readable archive. Reproduce slow behavior and attach that ZIP with the action/time; see [SUPPORT.md](SUPPORT.md).

Data lives under `~/Library/Application Support/io.github.iarnvidr.hrungnir/`. Structured content is `cache.sqlite`; completed media is in `media/`; website data belongs to the app's own webview store. Text pruning does not delete independently cached media or shrink SQLite allocation.

## Rebuild and Linux verification

On Mac with Tauri prerequisites: `npm ci`, prepare helpers with `python3 scripts/fetch-media-tools.py`, then `npm run build`. Release packaging from Linux uses `bash scripts/build-macos.sh` and a disposable SDK/zigbuild/rcodesign container.

Linux development needs yt-dlp, FFmpeg, GTK/WebKit and the Tauri build prerequisites. Native checks use Xvfb, OS clicks, screenshots, SQLite inspection and WebDriver. WebKitWebDriver can invalidate its inspector when a background source window is destroyed; the acquisition flow is therefore also checked with OS clicks and persisted results. Cached rendering/playback can be checked after a fresh process start without an acquisition window.

`scripts/live-smoke.py` is the historical 0.2.0 Live/Saved harness; it does not describe the 0.3.0 contract. `scripts/unified-smoke.py` exercises the current local reader/media flow.
