# Groups and the saved library

Subreddit groups control caching; Library collections organize saved items. Both live in SQLite and are shared by the app and CLI.

In **Settings → Subreddit groups**, create a named group and enter its subreddit names. Each subreddit belongs to one group. Set a post/unwatched-media lifetime and a watched-media lifetime in hours, or choose forever independently for either. Ungrouped posts and unwatched media use the global lifetime (48 hours by default). Text ages from capture, media from completed download. At 95% actual played coverage, videos instead expire 24 hours after first watch completion by default. Changing a group uses those original timestamps; cache hits and repeated bulk fetching do not reset media age. Keep video protects the post and media forever. Budget pressure may remove unprotected media earlier. Groups do not start background crawling or download every member automatically.

In a downloaded post, choose **Save post to Library**, select a collection and optionally **Keep post and its media forever**. New saves default to forever. Protected discussions and completed media survive automatic expiry, cache-budget pressure and **Clear cached posts**. Completed protected discussions are frozen against later automatic Reddit captures; an incomplete listing-only save can still receive its first discussion capture. Explicit backup restoration can replace content.

Forever items sit outside the evictable 10 MiB text/512 MiB media budgets. They consume disk until you remove their protection; exporting a backup is still necessary to protect against filesystem loss. Removing an item from Library releases its protection rather than immediately deleting its files. Non-forever saves follow their normal cache policies and disappear when expired/evicted.

## Articles, videos and photos

**Library → Add link** is available without an open post. The reader's **Save linked content…** action prefills the current post's target URL.

- **Article:** downloads public static HTML, extracts readable headings/paragraphs and retains up to six available raster images. Text and attribution are searchable; the original HTML/scripts are never rendered. This is a basic extractor, not full browser rendering or a complete page archive. JavaScript-only, login-only and paywalled pages can fail; unavailable optional images are skipped.
- **Video:** uses bundled yt-dlp and FFmpeg for supported public YouTube, Vimeo, Reddit, Streamin and Streamable links. Extraction is explicit, one item at a time, without browser profiles, credentials, cookies or playlists. Video helpers use their own network clients; this is not proxy response capture or a guarantee that every extractor URL is constrained by the article downloader's DNS filter. A complete local file is required before playback.
- **Image:** downloads a public HTTPS PNG, JPEG, GIF, WebP or AVIF file. SVG and HTML image viewers are not treated as direct photos.

The original URL is retained with each saved item. Article text is capped at 512 KiB; article images at 8 MiB each; standalone images/videos at 128 MiB each. Temporary linked media must fit the evictable media budget. HTTPS article/image requests reject private-network destinations, including redirects/resolution; selected video pages are limited to the providers above.

The Library has collections, type filters and ranked full-text search over saved titles, bodies and comments. Comment results open the matching cached comment. Opening saved content does not fetch the original site. List responses contain summaries; opening an item loads the full text and completed local files.

## Import and export

**Library → Export library…** writes a portable ZIP containing saved post/comment text, linked-article text, collection/forever metadata, subreddit groups and completed saved media. It excludes raw databases, CA keys, browser data, logs, UI preferences and watch-progress history. It is a library backup rather than a full application backup. Choose a new filename; existing files and managed application paths are never overwritten.

**Import library…** validates and stages the archive before committing. Identical existing media can be reused; conflicting media is rejected. Imports preserve existing forever protection. The archive limit is 2 GiB expanded, 10,000 entries and 128 MiB per file. Large libraries currently need smaller separately managed libraries/backups; there is no split-archive exporter yet.

The CLI also imports JSON URL lists and Netscape browser-bookmark HTML. Those imports create searchable, protected **bookmarks only**, with an explicit not-downloaded message. They never start bulk downloads. Use Add link/save-url to download a selected item afterward. ZIP backups preserve actual downloaded content; URL-list exports do not.

See [CLI.md](CLI.md) for headless commands and [MACOS.md](MACOS.md) for verification limits.
