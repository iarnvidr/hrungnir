# Hrungnir 0.6.2

- Settings now controls text size and element size independently, with preferences retained across restarts.
- Inline and reader media use the available width, preserving aspect ratio. Videos show a local first-frame JPEG before playback; existing caches generate it locally when opened.
- Each video has a **Keep video** action that protects its post and completed media forever, preserving its collection and current player.
- Bulk downloads now have an unwatched-media lifetime: 48 hours from download completion by default, overridden by the subreddit group's post/unwatched-media setting. At 95% actual watch coverage, the separate watched-media clock applies: 24 hours by default. Cache hits, repeated fetch-all and preview generation do not restart these clocks. Keep forever protects both; storage pressure can still evict unprotected media sooner.

125 Rust tests and native Linux build/Clippy pass. Native UI acceptance covers local preview decoding before playback, full-width media, playback/seek, inline protection, settings persistence and tab shortcuts. Apple Silicon build/signature/resource checks are recorded in the progress notes. Native macOS runtime verification remains pending.
