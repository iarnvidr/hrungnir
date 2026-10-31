# Hrungnir 0.6.15

- Compact the reader toolbar and make community navigation clearer.
- Improve recovery when subreddit and search requests overlap or fail, while preserving the current listing and cached discussions.
- Continue subreddit feeds across pages without dropping cached post discussions. Show Reddit's reported comment totals and let readers refresh a discussion explicitly.
- Start eligible Reddit video playback from a validated HLS source while saving the offline copy in the background. Keep streaming and completed offline media states separate, preserve the active player when the save finishes, and fall back to the local copy if streaming fails.
- Respect reduced-motion preferences.

Native macOS proof on one public Reddit video measured a decoded frame at 494 ms, playback at 495 ms, and a presented frame at 502 ms. This is one observed post and is not a guarantee of start time for other videos or connections. The container browser regression exercises the reader's explicit-play path; the native probe verified HLS playback separately and did not automate a native UI click.

Apple Silicon, macOS 14+. The app remains ad-hoc signed and not notarized; packaging is unchanged.

Verify the downloaded archive before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.15-macos-arm64.zip.sha256
```
