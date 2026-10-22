# Hrungnir 0.6.1

Fixes missing Streamff soccer videos and adds inline media fetching for a loaded subreddit listing.

- Streamff player links now resolve their public video metadata before invoking bundled yt-dlp. The reported Betis–Barcelona goal downloads as a completed local MP4; later opens reuse the cache.
- **Fetch all media** downloads media-bearing posts from the loaded listing and expands them inline. Progress, cancellation and per-post Retry are available. Two requests can be active; the existing native source and media queues bound the underlying work.
- Cmd+T opens a new browsing draft; Cmd+W closes the active draft/post tab. Native menu actions route to tabs rather than closing the app window.
- The tab strip retains keyboard/overflow navigation without a visible horizontal scrollbar. Repeating a batch preserves already loaded player elements.

The selected Streamff clip was verified at 14,875,302 bytes, followed by disk-only reuse. Native Linux inline playback and seeking pass for that 31.3-second clip; batch player preservation and native tab accelerators also pass. All 118 Rust tests and two packaging-policy tests pass, alongside native build/Clippy and ARM signature/resource checks. Native macOS runtime and playback verification remain pending. The app is ad-hoc signed, without Developer ID notarization.
