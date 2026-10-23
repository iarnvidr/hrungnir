# Hrungnir 0.6.7

- Posts open immediately using available content. Comments and media load independently, so video downloads no longer block reading.
- Linked articles appear inside opened posts as well as inline. Inline expansion preserves scroll position.
- Save post is now a single action, with collection and retention settings under Save options.
- Popular and All accept posts from multiple communities. Listing thumbnails survive post updates and load progressively.
- Open posts in background tabs using the button, Ctrl/Cmd-click, or middle-click.
- Post flair is shown and can filter a listing. Comment usernames and community flair labels are preserved; CSS-only club crest artwork is not included.
- Removed capture timestamps from listings and the reader. Cached listings remain usable during refresh, and late responses no longer move focus back from an opened post.

Apple Silicon, macOS 14+. The app is ad-hoc signed and not notarized.

Download the ZIP and checksum into the same folder, then verify before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.7-macos-arm64.zip.sha256
```

Expected output: `Hrungnir-0.6.7-macos-arm64.zip: OK`.

Validation includes hosted Linux and native macOS builds, media helper smoke tests, archive signature checks, parser regressions, and 12 browser checks with controlled native API fixtures. Browser fixtures verify UI behavior; they do not establish live Reddit availability or interactive native playback.
