# Hrungnir 0.5.4

Adds cached post/comment search, configurable post lifetime, the missing Streamin video path and cleanup 24 hours after a full watch.

- The header now searches this device's cache, with ranked title/body/comment matches, excerpts and comment anchors. Opening search results stays offline, including restored post tabs.
- Subreddit entry and Fetch controls move into the subreddit panel. Settings now includes post lifetime (1–8,760 hours, default 48); applying a shorter lifetime prunes expired posts and comments immediately.

- Recognizes `streamin.link` and `streamin.me` as video-page sources and passes them to the bundled yt-dlp. Their media CDN is `streamin.top`. Previously the host filter discarded the link before extraction, so no inline video control appeared.
- yt-dlp HLS/DASH downloads now use four concurrent fragments and one fragment retry, instead of the default single-fragment download and repeated failed-fragment retries. Single-file MP4 sources still download as one file.
- The reported clip was downloaded through the actual Rust media path with yt-dlp in 2.723 seconds on Linux: 19,207,438 bytes, H.264/AAC, 36.83 seconds. A second read reused the completed disk cache immediately. This is not a Mac speed guarantee.
- Matching Muninn's default, 95% played coverage marks a video fully watched; it expires 24 hours later. Seeking to the end does not count as watching. Progress persists across app restarts. Active playback is protected by renewable leases.

Cleanup runs on startup and every minute while the app is running. If the app is closed at expiry, removal happens on a later launch. For a post with multiple playable files, cleanup waits until all have completed their retention period. Existing cached files begin accumulating coverage after this update. Configured post/comment expiry is separate (default 48 hours and a 10 MiB logical budget); **Clear cached posts** clears structured content, not media. The 512 MiB media cap and 128 MiB per-file limit remain; budget pressure can still evict inactive media earlier.

Quit the old app before replacing it. Existing cached posts retain their link URLs, so this fix also applies to previously captured Streamin posts. Requires Apple Silicon/macOS 14+. Native macOS verification of this specific clip remains pending.

77 Rust tests, two packaging-policy tests, native Linux build/Clippy and the ARM cross-build pass. Native checks verify comment search/anchors, cached-only tabs across restart, post-lifetime application/pruning/persistence and zero source requests. Baseline-H.264 checks cover seek-without-credit, full watch, restart persistence and cleanup after test-record aging; structured posts remain. The ARM executable, helpers, signatures and resource hashes pass structural verification. High-profile/B-frame playback stalls in the headless Linux fixture and is not marked verified. Native Mac verification of this revision remains pending.
