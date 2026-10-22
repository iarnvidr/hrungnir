# Hrungnir 0.6.0

Adds subreddit groups and a protected, portable saved library, shared by the app and CLI.

- Settings now manages subreddit groups with independent post and watched-media lifetimes, including forever. The sidebar can filter by group.
- Save posts and their downloaded media into collections. Keep forever protects them from automatic expiry, budget eviction and cache clearing; completed discussions stay frozen against automatic source refreshes.
- Library saves selected public articles, videos and photos. Articles retain readable text and available images; supported video pages use bundled yt-dlp. Files play/read locally after download.
- Ranked Library search covers full saved text and comments, with comment anchors, collection/type filters and disk-only opening.
- Library ZIP import/export includes saved text, groups, metadata and completed media. URL-list/HTML bookmark imports are available through the headless CLI and do not start bulk downloads.
- The packaged executable now supports `--cli` for groups, saved items, linked downloads, search, import/export and diagnostics. Startup/list payloads use summaries; selected posts still load complete discussions.

See [library behavior](LIBRARY.md) and [CLI commands](CLI.md).

115 Rust tests, two packaging-policy tests, native Linux build/Clippy and ARM cross-build/signature/resource checks pass. Native UI checks cover protected media, group expiry, full-text comment anchors, GUI ZIP export/import, headless packaged CLI restoration, restart persistence and local opening of restored article/image/video content with zero Reddit requests. The PNG decodes; the video check covers completed local files/player controls rather than claiming playback of the known problematic High-profile fixture.

The article extractor handles static public HTML and best-effort images; login/JavaScript-only pages can fail. Video providers are initially YouTube, Vimeo, Reddit, Streamin and Streamable. Archive limits are 2 GiB expanded/128 MiB per file. Native macOS verification remains pending.
