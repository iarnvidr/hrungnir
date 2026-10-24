# Hrungnir 0.6.8

- Restore self-post bodies, including episode-link collections with zero comments. A listing's comment count no longer suppresses fetching the actual post, and collapsed body markup preserves paragraphs and link destinations.
- Click subreddit names to browse their community. Copy thread link and Open on Reddit are available in the post reader.
- Place post flair on the left beside the subreddit and align listing controls in consistent columns. Mobile listings remain full width.
- Only media expands inline. Linked articles remain available in the post reader.
- Fetch thumbnails independently from Reddit preview endpoints. Original full-size image URLs are excluded from thumbnail acquisition; preview loading does not initiate full media downloads.

Apple Silicon, macOS 14+. The app is ad-hoc signed and not notarized.

Download both release files to the same folder and verify before extracting:

```sh
shasum -a 256 -c Hrungnir-0.6.8-macos-arm64.zip.sha256
```

Expected: `Hrungnir-0.6.8-macos-arm64.zip: OK`.

Validation: 157 Rust tests pass (one network probe intentionally ignored), plus 12 browser checks covering the UI contract. Hosted Linux and native macOS checks compile the app; macOS automation exercises media helpers, signatures, the extracted archive and packaged CLI. Browser fixtures do not verify live Reddit access or native GUI playback.
