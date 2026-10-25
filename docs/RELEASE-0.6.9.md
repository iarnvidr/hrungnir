# Hrungnir 0.6.9

- Keep the subreddit navigator visible while scrolling posts. Long navigation lists scroll independently.
- Remember each subreddit's scroll position when switching communities, opening posts, visiting Library, or refreshing the listing.
- Put section navigation and open posts in one sticky tab row above the header, with the subreddit navigator first.
- Preserve post reading positions and keyboard focus while switching tabs. Prevent a delayed discussion redraw from resetting the current scroll position.

Apple Silicon, macOS 14+. The app is ad-hoc signed and not notarized.

Download both release files to the same folder and verify before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.9-macos-arm64.zip.sha256
```

Expected: `Hrungnir-0.6.9-macos-arm64.zip: OK`.

Validation: both reader browser suites passed against deterministic native API fixtures, including independent subreddit and post positions, refresh, keyboard navigation, and narrow layouts. JavaScript syntax checks and 12 packaging-policy tests passed in the dev environment. GitHub Actions runs Linux checks and native macOS compilation; the release workflow also verifies the archive checksum, signatures, media helpers, and extracted packaged CLI before publishing. Browser fixtures do not verify live Reddit access or native GUI playback.
